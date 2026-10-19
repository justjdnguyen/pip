// Drags a window's bottom-right corner quickly while recording the screen, then
// reports frames where the window background shows past the video's edge.
// Made for the native-video spike (avsb_spike draws a green border at the
// video's edge and has a black window background), but works for any window
// whose video has a bright green border.
//
// Usage: swift tools/drag-resize.swift <owner-name> [dx=300] [dy=169] [duration-s=0.6] [out.mov]
// Needs Accessibility (to post mouse events) and Screen Recording permission.
import AVFoundation
import CoreGraphics
import Foundation

let args = CommandLine.arguments
let owner = args.count > 1 ? args[1] : "avsb_spike"
let dx = args.count > 2 ? Double(args[2])! : 300
let dy = args.count > 3 ? Double(args[3])! : 169
let dur = args.count > 4 ? Double(args[4])! : 0.6
let out = args.count > 5 ? args[5] : "/tmp/drag-resize.mov"

let info = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
guard let win = info.first(where: { ($0[kCGWindowOwnerName as String] as? String) == owner }),
      let b = win[kCGWindowBounds as String] as? [String: Any],
      let x = b["X"] as? Double, let y = b["Y"] as? Double,
      let w = b["Width"] as? Double, let h = b["Height"] as? Double else {
  print("no window owned by \(owner)"); exit(1)
}
print("window \(x),\(y) \(w)x\(h)")

// Record the area the window will cover (plus a margin).
let rect = "\(Int(x)),\(Int(y)),\(Int(w + dx + 40)),\(Int(h + dy + 40))"
try? FileManager.default.removeItem(atPath: out)
let rec = Process()
rec.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
rec.arguments = ["-x", "-v", "-V", String(Int(dur + 3)), "-R", rect, out]
try rec.run()
Thread.sleep(forTimeInterval: 1.0)

// Drag from just inside the corner.
let src = CGEventSource(stateID: .hidSystemState)
let start = CGPoint(x: x + w - 3, y: y + h - 3)
func post(_ type: CGEventType, _ p: CGPoint) {
  CGEvent(mouseEventSource: src, mouseType: type, mouseCursorPosition: p, mouseButton: .left)?.post(tap: .cghidEventTap)
}
post(.mouseMoved, start); Thread.sleep(forTimeInterval: 0.1)
post(.leftMouseDown, start); Thread.sleep(forTimeInterval: 0.05)
let steps = Int(dur * 120)
for i in 1...steps {
  // Out and back, so both growing and shrinking are covered.
  let f = Double(i) / Double(steps)
  let k = f < 0.5 ? f * 2 : 2 - f * 2
  post(.leftMouseDragged, CGPoint(x: start.x + dx * k, y: start.y + dy * k))
  Thread.sleep(forTimeInterval: dur / Double(steps))
}
post(.leftMouseUp, start)
rec.waitUntilExit()

// Per frame: bounding box of the green border, then count pure-black pixels
// just right of / below it (window background showing = a gap).
let asset = AVURLAsset(url: URL(fileURLWithPath: out))
let sema = DispatchSemaphore(value: 0)
Task {
  let track = try await asset.loadTracks(withMediaType: .video).first!
  let reader = try AVAssetReader(asset: asset)
  let o = AVAssetReaderTrackOutput(track: track, outputSettings: [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA])
  reader.add(o); reader.startReading()
  var frames = 0, gapFrames = 0, worst = 0
  var last: [UInt8] = []
  while let sb = o.copyNextSampleBuffer(), let pb = CMSampleBufferGetImageBuffer(sb) {
    CVPixelBufferLockBaseAddress(pb, .readOnly)
    let base = CVPixelBufferGetBaseAddress(pb)!.assumingMemoryBound(to: UInt8.self)
    let rowBytes = CVPixelBufferGetBytesPerRow(pb), W = CVPixelBufferGetWidth(pb), H = CVPixelBufferGetHeight(pb)
    func px(_ x: Int, _ y: Int) -> (Int, Int, Int) { let p = base + y * rowBytes + x * 4; return (Int(p[2]), Int(p[1]), Int(p[0])) }
    var sig = [UInt8](); var i = 0; while i < rowBytes * H { sig.append(base[i]); i += 4999 }
    if sig == last { CVPixelBufferUnlockBaseAddress(pb, .readOnly); continue }  // duplicate frame
    last = sig
    var maxX = -1, maxY = -1
    for yy in stride(from: 0, to: H, by: 2) { for xx in stride(from: 0, to: W, by: 2) {
      let (r, g, bl) = px(xx, yy); if g > 190 && r < 80 && bl < 130 { maxX = max(maxX, xx); maxY = max(maxY, yy) }
    } }
    var black = 0
    if maxX >= 0 {
      for yy in 0..<min(H, maxY) { for xx in (maxX + 2)..<min(W, maxX + 40) { let (r, g, bl) = px(xx, yy); if r < 6 && g < 6 && bl < 6 { black += 1 } } }
      for yy in (maxY + 2)..<min(H, maxY + 40) { for xx in 0..<min(W, maxX) { let (r, g, bl) = px(xx, yy); if r < 6 && g < 6 && bl < 6 { black += 1 } } }
    }
    CVPixelBufferUnlockBaseAddress(pb, .readOnly)
    frames += 1
    // A few stray pixels can be the window's rounded corner/shadow; call >200 a gap.
    if black > 200 { gapFrames += 1 }
    worst = max(worst, black)
  }
  print("distinct frames: \(frames), frames with a gap: \(gapFrames), worst gap: \(worst) px  (\(out))")
  sema.signal()
}
sema.wait()
