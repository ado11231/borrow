import SwiftUI

/// Healthy values stay plain, as on the command line. Color is for what needs attention.
func valueColor(_ level: Level?) -> Color {
    switch level {
    case .warning: .orange
    case .high: .red
    default: .primary
    }
}

func graphColor(_ level: Level?) -> Color {
    switch level {
    case .warning: .orange
    case .high: .red
    default: .accentColor
    }
}

struct PopoverView: View {
    let watcher: Watcher
    @State private var startsAtLogin = LoginItem.enabled

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            header
            Divider()
            if let status = watcher.status, status.online {
                metrics(status)
            } else {
                offline
            }
            Divider()
            footer
        }
        .padding(14)
        .frame(width: 300)
    }

    private var header: some View {
        HStack(spacing: 8) {
            Circle()
                .fill(watcher.status?.online == true ? Color.green : Color.secondary)
                .frame(width: 8, height: 8)
            Text(watcher.status?.agent.nonEmpty ?? "Slingshot")
                .font(.headline)
            Spacer()
            if let path = watcher.status?.path {
                Text("via \(path)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }

    @ViewBuilder
    private func metrics(_ status: Status) -> some View {
        if let cpu = status.cpu {
            GraphRow(
                label: "CPU",
                value: String(format: "%.0f%%", cpu.percent),
                level: cpu.level,
                history: watcher.cpuHistory
            )
        }
        if let memory = status.memory {
            BarRow(
                label: "RAM",
                value: "\(capacity(memory.usedMib)) / \(capacity(memory.totalMib))",
                usage: memory
            )
        }
        ForEach(Array(status.gpus.enumerated()), id: \.offset) { index, gpu in
            gpuRows(gpu, history: index < watcher.gpuHistory.count ? watcher.gpuHistory[index] : [])
        }
        if let problem = status.gpuProblem {
            Text(problem)
                .font(.caption)
                .foregroundStyle(.orange)
        }
        if let workspace = status.workspace {
            HStack {
                Text("Workspace").font(.caption).foregroundStyle(.secondary)
                Spacer()
                Text("\(capacity(workspace.freeMib)) free")
                    .font(.system(.callout, design: .rounded).monospacedDigit())
                    .foregroundStyle(valueColor(workspace.level))
            }
        }
    }

    @ViewBuilder
    private func gpuRows(_ gpu: Gpu, history: [Double]) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            GraphRow(
                label: "GPU",
                detail: gpu.name,
                value: gpuValue(gpu),
                level: worst(gpu.utilization?.level, gpu.temperatureLevel),
                history: history
            )
            if let vram = gpu.vram {
                BarRow(
                    label: "VRAM",
                    value: "\(capacity(vram.usedMib)) / \(capacity(vram.totalMib))",
                    usage: vram
                )
            }
        }
    }

    private func gpuValue(_ gpu: Gpu) -> String {
        [
            gpu.utilization.map { String(format: "%.0f%%", $0.percent) },
            gpu.temperatureC.map { "\($0)°C" },
        ]
        .compactMap { $0 }
        .joined(separator: " · ")
    }

    private func worst(_ a: Level?, _ b: Level?) -> Level? {
        [a, b].compactMap { $0 }.max { rank($0) < rank($1) }
    }

    private func rank(_ level: Level) -> Int {
        switch level {
        case .good: 0
        case .warning: 1
        case .high: 2
        }
    }

    private var offline: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(watcher.status == nil && watcher.problem == nil ? "Connecting…" : "Not connected")
                .font(.callout.weight(.medium))
            if let reason = watcher.problem ?? watcher.status?.error {
                Text(reason)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
            }
        }
    }

    private var footer: some View {
        HStack {
            Toggle("Open at login", isOn: $startsAtLogin)
                .toggleStyle(.checkbox)
                .font(.caption)
                .onAppear { startsAtLogin = LoginItem.enabled }
                .onChange(of: startsAtLogin) { _, on in
                    if on != LoginItem.enabled { LoginItem.set(on) }
                }
            Spacer()
            Button("Quit") { NSApplication.shared.terminate(nil) }
                .buttonStyle(.borderless)
                .font(.caption)
                .keyboardShortcut("q")
        }
    }
}

struct GraphRow: View {
    let label: String
    var detail: String? = nil
    let value: String
    let level: Level?
    let history: [Double]

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .firstTextBaseline) {
                Text(label).font(.caption).foregroundStyle(.secondary)
                if let detail {
                    Text(detail)
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                        .lineLimit(1)
                }
                Spacer()
                Text(value)
                    .font(.system(.callout, design: .rounded).monospacedDigit())
                    .foregroundStyle(valueColor(level))
            }
            Sparkline(values: history, color: graphColor(level))
                .frame(height: 28)
        }
    }
}

struct BarRow: View {
    let label: String
    let value: String
    let usage: Usage

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(label).font(.caption).foregroundStyle(.secondary)
                Spacer()
                Text(value)
                    .font(.system(.callout, design: .rounded).monospacedDigit())
                    .foregroundStyle(valueColor(usage.level))
            }
            UsageBar(fraction: usage.fraction, color: graphColor(usage.level))
        }
    }
}

extension String {
    var nonEmpty: String? { isEmpty ? nil : self }
}
