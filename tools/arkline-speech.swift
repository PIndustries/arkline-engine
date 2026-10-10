// Native child-process speech bridge. No audio/transcripts are persisted.
// Build: swiftc tools/arkline-speech.swift -o arkline-speech
// Bundle Info.plist must supply NSMicrophoneUsageDescription and
// NSSpeechRecognitionUsageDescription before explicitly invoking --listen.
import Foundation
import AVFoundation
import Speech

final class SpeechBridge: NSObject, AVSpeechSynthesizerDelegate {
    var done = false
    var exitCode: Int32 = 0
    var engine: AVAudioEngine?
    var request: SFSpeechAudioBufferRecognitionRequest?
    var task: SFSpeechRecognitionTask?
    var recognizer: SFSpeechRecognizer?
    let synthesizer = AVSpeechSynthesizer()
    var tapInstalled = false
    var finishing = false
    var speechInputReceived = false
    var controls = Data()
    var lastPartial = ""

    func emit(_ event: String, _ fields: [String: Any] = [:]) {
        var object = fields
        object["event"] = event
        if let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]) {
            FileHandle.standardOutput.write(data)
            FileHandle.standardOutput.write(Data([10]))
        }
    }
    func finish(_ event: String, _ fields: [String: Any] = [:], code: Int32 = 0) {
        guard !done else { return }
        done = true
        exitCode = code
        stopCapture()
        task?.cancel()
        FileHandle.standardInput.readabilityHandler = nil
        emit(event, fields)
    }
    func fail(_ message: String) { finish("error", ["message": message], code: 1) }
    func stopCapture() {
        engine?.stop()
        if tapInstalled { engine?.inputNode.removeTap(onBus: 0); tapInstalled = false }
        request?.endAudio()
    }
    func stopListening() {
        guard !done, !finishing else { return }
        finishing = true
        stopCapture()
        task?.finish()
        // A partial result is never relabelled as a confirmed final transcript.
        DispatchQueue.main.asyncAfter(deadline: .now() + 3) { [weak self] in
            guard let self, !self.done else { return }
            self.fail("Recognition did not produce a final result after listening stopped")
        }
    }
    func listenControls() {
        FileHandle.standardInput.readabilityHandler = { [weak self] handle in
            let data = handle.availableData
            DispatchQueue.main.async {
                guard let self, !self.done else { return }
                if data.isEmpty { self.fail("Listening cancelled: controller input closed"); return }
                self.controls.append(data)
                if self.controls.count > 4096 { self.fail("Invalid oversized speech control input"); return }
                while let newline = self.controls.firstIndex(of: 10) {
                    let line = String(decoding: self.controls[..<newline], as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
                    self.controls.removeSubrange(...newline)
                    switch line {
                    case "stop": self.stopListening()
                    case "cancel": self.fail("Listening cancelled")
                    case "": break
                    default: self.fail("Unknown speech control command")
                    }
                }
            }
        }
    }
    func authorize(locale: String, microphone: Bool, start: @escaping (SFSpeechRecognizer) -> Void) {
        guard Bundle.main.object(forInfoDictionaryKey: "NSSpeechRecognitionUsageDescription") != nil else {
            fail("Speech recognition requires the packaged app's speech usage description"); return
        }
        if microphone && Bundle.main.object(forInfoDictionaryKey: "NSMicrophoneUsageDescription") == nil {
            fail("Microphone capture requires the packaged app's microphone usage description"); return
        }
        guard let recognizer = SFSpeechRecognizer(locale: Locale(identifier: locale)),
              recognizer.supportsOnDeviceRecognition else {
            fail("On-device speech recognition is unavailable for this locale; no cloud fallback is allowed"); return
        }
        self.recognizer = recognizer
        SFSpeechRecognizer.requestAuthorization { [weak self] status in
            DispatchQueue.main.async {
                guard let self, !self.done else { return }
                guard status == .authorized else { self.fail("Speech recognition permission was not granted"); return }
                if microphone {
                    AVCaptureDevice.requestAccess(for: .audio) { allowed in
                        DispatchQueue.main.async {
                            guard !self.done else { return }
                            if allowed { start(recognizer) } else { self.fail("Microphone permission was not granted") }
                        }
                    }
                } else { start(recognizer) }
            }
        }
    }
    func recognition(_ recognizer: SFSpeechRecognizer, request: SFSpeechRecognitionRequest) {
        request.requiresOnDeviceRecognition = true
        request.shouldReportPartialResults = true
        request.taskHint = .dictation
        task = recognizer.recognitionTask(with: request) { [weak self] result, error in
            DispatchQueue.main.async {
                guard let self, !self.done else { return }
                if let result {
                    let text = result.bestTranscription.formattedString
                    guard text.count <= 2000 else { self.fail("Transcript exceeded 2000 characters"); return }
                    if result.isFinal { self.finish("final", ["text": text]); return }
                    if text != self.lastPartial { self.lastPartial = text; self.emit("partial", ["text": text]) }
                }
                if let error { self.fail("On-device recognition failed: \(error.localizedDescription)") }
            }
        }
    }
    func listen(locale: String) {
        listenControls()
        authorize(locale: locale, microphone: true) { [weak self] recognizer in
            guard let self, !self.done, !self.finishing else { return }
            let engine = AVAudioEngine()
            let request = SFSpeechAudioBufferRecognitionRequest()
            self.engine = engine
            self.request = request
            let input = engine.inputNode
            let format = input.outputFormat(forBus: 0)
            guard format.sampleRate > 0, format.channelCount > 0 else { self.fail("No usable microphone input device"); return }
            self.recognition(recognizer, request: request)
            input.installTap(onBus: 0, bufferSize: 1024, format: format) { buffer, _ in request.append(buffer) }
            self.tapInstalled = true
            engine.prepare()
            do { try engine.start() } catch { self.fail("Microphone could not start: \(error.localizedDescription)"); return }
            self.emit("ready", ["mode": "listen", "on_device": true])
            DispatchQueue.main.asyncAfter(deadline: .now() + 20) { [weak self] in self?.stopListening() }
        }
    }
    func transcribe(path: String, locale: String) {
        let url = URL(fileURLWithPath: path)
        guard FileManager.default.fileExists(atPath: url.path) else { fail("Audio fixture does not exist"); return }
        authorize(locale: locale, microphone: false) { [weak self] recognizer in
            guard let self, !self.done else { return }
            self.recognition(recognizer, request: SFSpeechURLRecognitionRequest(url: url))
            self.emit("ready", ["mode": "transcribe-file", "on_device": true])
            DispatchQueue.main.asyncAfter(deadline: .now() + 20) { [weak self] in self?.fail("File transcription exceeded 20 seconds") }
        }
    }
    func speak(text: String, voice: String?, locale: String) {
        guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { fail("Speech text is empty"); return }
        guard text.count <= 2000 else { fail("Speech text exceeded 2000 characters"); return }
        let utterance = AVSpeechUtterance(string: text)
        if let voice {
            guard let selected = AVSpeechSynthesisVoice(identifier: voice) else { fail("Selected speech voice is unavailable"); return }
            utterance.voice = selected
        } else { utterance.voice = AVSpeechSynthesisVoice(language: locale) }
        synthesizer.delegate = self
        emit("ready", ["mode": "speak"])
        synthesizer.speak(utterance)
        DispatchQueue.main.asyncAfter(deadline: .now() + 60) { [weak self] in
            guard let self, !self.done else { return }
            self.synthesizer.stopSpeaking(at: .immediate)
            self.fail("Speech playback exceeded 60 seconds")
        }
    }
    func speechSynthesizer(_ synthesizer: AVSpeechSynthesizer, didFinish utterance: AVSpeechUtterance) { finish("final", ["mode": "speak"]) }
    func speechSynthesizer(_ synthesizer: AVSpeechSynthesizer, didCancel utterance: AVSpeechUtterance) { fail("Speech playback cancelled") }
}

let bridge = SpeechBridge()
let args = Array(CommandLine.arguments.dropFirst())
func value(after flag: String) -> String? {
    guard let i = args.firstIndex(of: flag), i + 1 < args.count else { return nil }
    return args[i+1]
}
let locale = value(after: "--locale") ?? "en-US"
switch args.first {
case "--listen": bridge.listen(locale: locale)
case "--transcribe-file":
    if let path = value(after: "--transcribe-file") { bridge.transcribe(path: path, locale: locale) }
    else { bridge.fail("--transcribe-file requires a local audio path") }
case "--speak":
    // Bound memory even when the caller never closes its pipe. The parent also
    // imposes a process deadline; no microphone permission is requested here.
    DispatchQueue.global(qos: .userInitiated).async {
        var bytes = Data()
        while bytes.count <= 8192 {
            let chunk = FileHandle.standardInput.readData(ofLength: min(1024,8193-bytes.count))
            if chunk.isEmpty { break }
            bytes.append(chunk)
        }
        let input = bytes
        DispatchQueue.main.async {
            guard !bridge.done else { return }
            bridge.speechInputReceived = true
            if input.count > 8192 { bridge.fail("Speech input exceeded 8192 bytes") }
            else if let text = String(data: input, encoding: .utf8) { bridge.speak(text: text, voice: value(after: "--voice"), locale: locale) }
            else { bridge.fail("Speech input must be UTF-8") }
        }
    }
    DispatchQueue.main.asyncAfter(deadline: .now() + 5) {
        if !bridge.done && !bridge.speechInputReceived {
            bridge.fail("Speech input was not completed within 5 seconds")
        }
    }
case "--voices":
    bridge.finish("final", ["voices": AVSpeechSynthesisVoice.speechVoices().map { ["id": $0.identifier, "name": $0.name, "language": $0.language] }])
default: bridge.fail("Usage: --listen | --speak | --voices | --transcribe-file PATH [--locale en-US] [--voice ID]")
}
// Permission prompts are bounded independently from the 20-second capture timer.
let absoluteDeadline = Date().addingTimeInterval(90)
while !bridge.done && Date() < absoluteDeadline { RunLoop.main.run(until: Date().addingTimeInterval(0.05)) }
if !bridge.done { bridge.fail("Speech helper timed out") }
exit(bridge.exitCode)
