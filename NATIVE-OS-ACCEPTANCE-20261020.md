# Native OS acceptance and bundle-version checkpoint

Build **26.10.19.2** is still running as PID26743 on the disposable ReconstructionQA project. This record describes real macOS mouse/key events and owned-window captures separately from the earlier automated CGL tests.

| Journey | Result |
| --- | --- |
| Window title, build stamp and disposable project identity | Passed |
| OS Shift+Left / Shift+Right, 0.8second holds, then release | Passed; opposite rotations and recentering captured; no crash |
| Mounted instrument perspective/visibility | Panels move with the view and return at centre; controlled OS depth/Shift-alone live-update probe incomplete |
| OS Play, physical Flight/Drones tab clicks and Undock | Passed; visible UI and authority state changed |
| Recon launch and physical camera display | Passed; Recon1 visible in world, fleet and camera |
| Text entry/focus, continuous-feed freshness and further travel controls | Unavailable after the editor lost foreground ownership; input guard stopped before typing |
| Ordinary opening | Minimal `open -a` passed with the same running instance |
| Browser GPU runtime | Unverified; no supported callable route available |

The broad read-only accessibility-tree scan was terminated by its60second guard. Later foreground checks rejected input before posting it; normal AppKit activation did not re-establish editor foreground ownership. No lock, permission or security restriction was bypassed. No text fixture was submitted, all posted keys were released, and the running editor was not forced closed. The disposable world remains playing with one Recon.

The initial `open -n` invocation with redirection/environment options failed-10810. Minimal ordinary opening later succeeded. This does not establish which option or session condition caused the earlier failure, and it does not prove cold launching a different bundle.

## Version correction

The old `CFBundleVersion20261019.2` exceeded Apple's documented numeric component widths. The shared build helper and Mac packager now use calendar days since2020-01-01 plus1, followed by the daily sequence split into two base100 components. Thus the current build maps to **2484.0.2**, and historical sequence134 maps to2484.1.34. Unsupported date/count limits fail explicitly. The human-facing `ArklineBuildVersion` and `BUILD_VERSION` remain26.10.19.2.

All14 build/version tests, all8 offline packaging tests and shell syntax pass. A new metadata-corrected app was packaged and deep/strict signature-verified at `.build/packages/reconstruction-native-metadata-20261020/Arkline Editor Reconstructed.app`. It contains the same pre-signing executable as the previously validated native build. The original running bundle is preserved. The corrected bundle is prepared, but a cold-launch swap was not forced while foreground ownership was unavailable.

Both old and corrected bundles are recognized by CoreFoundation. Its legacy `CFBundleGetVersionNumber` returns0 for both and is not used as proof of modern version validity or launch failure; the component limits and actual opening result are recorded separately.

The build was stamped at04:40UTC on October20, still October19 in the repository's **America/Chicago** build-date policy. This new checkout allocated local sequence1, then2 after the GL fix. It is a new reconstruction, not recovered original build46.

Evidence files, exact event logs, hashes and individual limitations are in [NATIVE-OS-ACCEPTANCE-20261020.json](NATIVE-OS-ACCEPTANCE-20261020.json). Captures are under `.build/packages/reconstruction-native-fixed-20261020/os-ui-acceptance-20261020/`. No existing user project, security/quarantine setting, remote repository or external drive was changed.
