// Counts distinct frames per second in a screen recording (to check what
// actually reached the display). Usage: swift tools/count-frames.swift file.mov
import AVFoundation
import Foundation

let url = URL(fileURLWithPath: CommandLine.arguments[1])
let asset = AVURLAsset(url: url)
let sema = DispatchSemaphore(value: 0)
Task {
  let track = try await asset.loadTracks(withMediaType: .video).first!
  let reader = try AVAssetReader(asset: asset)
  let out = AVAssetReaderTrackOutput(track: track, outputSettings: [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA])
  reader.add(out); reader.startReading()
  var last: [UInt8] = []; var total = 0; var distinct = 0; var first: Double? = nil; var lastT = 0.0
  while let sb = out.copyNextSampleBuffer(), let pb = CMSampleBufferGetImageBuffer(sb) {
    let t = CMSampleBufferGetPresentationTimeStamp(sb).seconds
    if first == nil { first = t }; lastT = t
    CVPixelBufferLockBaseAddress(pb, .readOnly)
    let base = CVPixelBufferGetBaseAddress(pb)!.assumingMemoryBound(to: UInt8.self)
    let n = CVPixelBufferGetBytesPerRow(pb) * CVPixelBufferGetHeight(pb)
    // Sample every 997th byte: cheap fingerprint of the frame.
    var sig = [UInt8](); sig.reserveCapacity(n / 997 + 1)
    var i = 0; while i < n { sig.append(base[i]); i += 997 }
    CVPixelBufferUnlockBaseAddress(pb, .readOnly)
    total += 1; if sig != last { distinct += 1; last = sig }
  }
  let secs = max(0.001, lastT - (first ?? 0))
  print(String(format: "recorded frames: %d, distinct: %d over %.1fs -> %.1f distinct fps", total, distinct, secs, Double(distinct) / secs))
  sema.signal()
}
sema.wait()
