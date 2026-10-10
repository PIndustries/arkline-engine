// Original Arkline orbital monogram. Run: swift tools/render-icon.swift OUTPUT.png
import AppKit
let size = 1024
let image = NSImage(size: NSSize(width:size,height:size))
image.lockFocus()
let ctx = NSGraphicsContext.current!.cgContext
func color(_ r:CGFloat,_ g:CGFloat,_ b:CGFloat,_ a:CGFloat=1)->NSColor { NSColor(calibratedRed:r,green:g,blue:b,alpha:a) }
func polygon(_ points:[CGPoint],_ fill:NSColor) {
 let p=NSBezierPath();p.move(to:points[0]);for pt in points.dropFirst(){p.line(to:pt)};p.close();fill.setFill();p.fill()
}
let background=NSBezierPath(roundedRect:NSRect(x:30,y:30,width:964,height:964),xRadius:212,yRadius:212)
NSGradient(colors:[color(0.07,0.15,0.20),color(0.015,0.03,0.07)])!.draw(in:background,angle:270)
color(0.18,0.35,0.41).setStroke();background.lineWidth=5;background.stroke()
// Orbital tracks behind the forged A silhouette.
ctx.saveGState();ctx.translateBy(x:512,y:482);ctx.rotate(by:CGFloat.pi/7)
let orbit=NSBezierPath(ovalIn:NSRect(x:-391,y:-166,width:782,height:332))
color(0.22,0.90,0.45,0.16).setStroke();orbit.lineWidth=24;orbit.stroke()
color(0.30,0.92,0.50,0.9).setStroke();orbit.lineWidth=7;orbit.stroke()
ctx.restoreGState()
// Faceted letterform with broad silhouette readable in the Dock.
polygon([CGPoint(x:229,y:254),CGPoint(x:449,y:786),CGPoint(x:516,y:820),CGPoint(x:516,y:679),CGPoint(x:359,y:254)],color(0.78,0.88,0.91))
polygon([CGPoint(x:516,y:820),CGPoint(x:798,y:254),CGPoint(x:666,y:254),CGPoint(x:516,y:594)],color(0.29,0.90,0.48))
polygon([CGPoint(x:516,y:820),CGPoint(x:549,y:736),CGPoint(x:760,y:254),CGPoint(x:798,y:254)],color(0.10,0.48,0.28))
polygon([CGPoint(x:347,y:351),CGPoint(x:399,y:465),CGPoint(x:613,y:465),CGPoint(x:670,y:351)],color(0.56,0.70,0.77))
polygon([CGPoint(x:362,y:371),CGPoint(x:410,y:448),CGPoint(x:605,y:448),CGPoint(x:646,y:371)],color(0.10,0.26,0.32))
// Navigation star, offset on orbit, adds an unmistakable space identity.
let star=[CGPoint(x:807,y:717),CGPoint(x:823,y:668),CGPoint(x:872,y:652),CGPoint(x:823,y:636),CGPoint(x:807,y:587),CGPoint(x:791,y:636),CGPoint(x:742,y:652),CGPoint(x:791,y:668)]
ctx.setShadow(offset:.zero,blur:24,color:color(0.27,1,0.56,0.7).cgColor)
polygon(star,color(0.77,1,0.84));ctx.setShadow(offset:.zero,blur:0,color:nil)
image.unlockFocus()
let rep=NSBitmapImageRep(data:image.tiffRepresentation!)!
try rep.representation(using:.png,properties:[:])!.write(to:URL(fileURLWithPath:CommandLine.arguments[1]))
