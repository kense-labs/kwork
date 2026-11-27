// Return the normalized center of visible text in a native-window screenshot.
import Foundation
import Vision

let image = URL(fileURLWithPath: CommandLine.arguments[1])
let label = CommandLine.arguments[2]
// Vision may omit the space between a Latin word and a Chinese label.
let pattern = label.split(whereSeparator: \.isWhitespace)
    .map { NSRegularExpression.escapedPattern(for: String($0)) }
    .joined(separator: "\\s*")
let request = VNRecognizeTextRequest()
request.recognitionLevel = .accurate
request.recognitionLanguages = ["zh-Hans", "en-US"]
request.usesLanguageCorrection = false
try VNImageRequestHandler(url: image).perform([request])
for result in (request.results ?? []).sorted(by: { $0.boundingBox.midY < $1.boundingBox.midY }) {
    guard let line = result.topCandidates(1).first,
          (label != "允许" || line.string.trimmingCharacters(in: .whitespacesAndNewlines) == label),
          let range = line.string.range(of: pattern, options: [.caseInsensitive, .regularExpression]),
          let box = try line.boundingBox(for: range) else {continue}
    print(box.boundingBox.midX, 1 - box.boundingBox.midY)
    exit(0)
}
FileHandle.standardError.write(Data("Missing visible text: \(label)\n".utf8))
exit(1)
