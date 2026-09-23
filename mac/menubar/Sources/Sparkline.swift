import SwiftUI

/// A small line graph of percentages, oldest on the left, with a soft fill underneath.
struct Sparkline: View {
    let values: [Double]
    let color: Color
    var capacity = Watcher.historyLength

    var body: some View {
        GeometryReader { geometry in
            let points = points(in: geometry.size)
            ZStack {
                if let first = points.first, let last = points.last {
                    Path { path in
                        path.move(to: CGPoint(x: first.x, y: geometry.size.height))
                        points.forEach { path.addLine(to: $0) }
                        path.addLine(to: CGPoint(x: last.x, y: geometry.size.height))
                        path.closeSubpath()
                    }
                    .fill(LinearGradient(
                        colors: [color.opacity(0.35), color.opacity(0.02)],
                        startPoint: .top,
                        endPoint: .bottom
                    ))
                    Path { path in path.addLines(points) }
                        .stroke(color, style: StrokeStyle(lineWidth: 1.5, lineCap: .round, lineJoin: .round))
                }
            }
        }
    }

    /// New samples enter on the right, so a short history sits against the right edge.
    private func points(in size: CGSize) -> [CGPoint] {
        guard values.count > 1 else { return [] }
        let step = size.width / CGFloat(max(capacity - 1, 1))
        let start = size.width - step * CGFloat(values.count - 1)
        return values.enumerated().map { index, value in
            let clamped = min(max(value, 0), 100) / 100
            return CGPoint(
                x: start + step * CGFloat(index),
                y: size.height - CGFloat(clamped) * (size.height - 2) - 1
            )
        }
    }
}

/// A thin filled bar for used capacity.
struct UsageBar: View {
    let fraction: Double
    let color: Color

    var body: some View {
        GeometryReader { geometry in
            ZStack(alignment: .leading) {
                Capsule().fill(.quaternary)
                Capsule()
                    .fill(color)
                    .frame(width: geometry.size.width * min(max(fraction, 0), 1))
            }
        }
        .frame(height: 6)
    }
}
