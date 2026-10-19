// Screenshots only the Better PiP app's windows (nothing else on screen).
// Usage: swift tools/capture-app-windows.swift <output-prefix>
import CoreGraphics
import Foundation

let prefix = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "/tmp/bp-window"
let info = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
var n = 0
for w in info where (w[kCGWindowOwnerName as String] as? String)?.contains("better-pip-host") == true
    || (w[kCGWindowOwnerName as String] as? String) == "Better PiP" {
  guard let id = w[kCGWindowNumber as String] as? Int else { continue }
  let b = w[kCGWindowBounds as String] as? [String: Any] ?? [:]
  let out = "\(prefix)-\(n).png"
  let p = Process()
  p.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
  p.arguments = ["-x", "-o", "-l", String(id), out]
  try? p.run(); p.waitUntilExit()
  print("window \(id) bounds=\(b) -> \(out)")
  n += 1
}
if n == 0 { print("no Better PiP windows on screen") }
