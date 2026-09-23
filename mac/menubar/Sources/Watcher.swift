import Foundation
import Observation

/// Runs `slingshot internal-watch` and keeps what it reports, plus a short history for the
/// graphs. Closing the helper's input is what stops it, so it never outlives the app.
@MainActor
@Observable
final class Watcher {
    /// About two minutes at one sample every two seconds.
    static let historyLength = 60

    private(set) var status: Status?
    /// Why there is nothing to show, such as a missing program or a helper that keeps failing.
    private(set) var problem: String?
    private(set) var cpuHistory: [Double] = []
    private(set) var gpuHistory: [[Double]] = []

    var onNotice: ((Notice) -> Void)?

    private var process: Process?
    private var input: Pipe?
    private var buffer = Data()
    private var errors = Data()
    private var restarts = 0
    private var stopping = false
    private var started: (program: String, agent: String?)?

    private static let backoff: [TimeInterval] = [2, 5, 15, 30]

    /// Start the helper, or restart it when `slingshot menubar` saved a new program or box.
    func start() {
        let defaults = UserDefaults.standard
        guard let program = defaults.string(forKey: "slingshotPath"),
              FileManager.default.isExecutableFile(atPath: program)
        else {
            problem = "Run slingshot menubar once from a terminal so the app can find Slingshot."
            return
        }
        let agent = defaults.string(forKey: "agent")
        if process?.isRunning == true, started?.program == program, started?.agent == agent {
            return
        }
        stop()
        stopping = false
        launch(program: program, agent: agent)
    }

    func stop() {
        stopping = true
        try? input?.fileHandleForWriting.close()
        process = nil
        input = nil
    }

    private func launch(program: String, agent: String?) {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: program)
        var arguments = ["--color", "never"]
        if let agent { arguments += ["--agent", agent] }
        process.arguments = arguments + ["internal-watch"]

        let input = Pipe()
        let output = Pipe()
        let errorOutput = Pipe()
        process.standardInput = input
        process.standardOutput = output
        process.standardError = errorOutput

        output.fileHandleForReading.readabilityHandler = { [weak self] handle in
            let data = handle.availableData
            Task { @MainActor in self?.received(data) }
        }
        errorOutput.fileHandleForReading.readabilityHandler = { [weak self] handle in
            let data = handle.availableData
            Task { @MainActor in self?.errors.append(data) }
        }
        process.terminationHandler = { [weak self] _ in
            Task { @MainActor in self?.ended(process) }
        }

        buffer = Data()
        errors = Data()
        do {
            try process.run()
        } catch {
            problem = "Could not start Slingshot at \(program): \(error.localizedDescription)"
            return
        }
        self.process = process
        self.input = input
        started = (program, agent)
    }

    private func received(_ data: Data) {
        buffer.append(data)
        while let newline = buffer.firstIndex(of: UInt8(ascii: "\n")) {
            let line = buffer[buffer.startIndex..<newline]
            buffer.removeSubrange(buffer.startIndex...newline)
            guard let event = decodeEvent(Data(line)) else { continue }
            switch event {
            case .status(let status): apply(status)
            case .notice(let notice): onNotice?(notice)
            case .unsupported(let version):
                problem = "Slingshot speaks format \(version), this app speaks \(supportedVersion). Update the app with mac/menubar/build.sh."
            }
        }
    }

    private func apply(_ status: Status) {
        self.status = status
        problem = nil
        restarts = 0
        guard status.online else { return }
        cpuHistory = appended(cpuHistory, status.cpu?.percent ?? 0)
        if gpuHistory.count != status.gpus.count {
            gpuHistory = Array(repeating: [], count: status.gpus.count)
        }
        for (index, gpu) in status.gpus.enumerated() {
            gpuHistory[index] = appended(gpuHistory[index], gpu.utilization?.percent ?? 0)
        }
    }

    private func appended(_ history: [Double], _ value: Double) -> [Double] {
        Array((history + [value]).suffix(Self.historyLength))
    }

    /// The helper only exits on its own when something is wrong, so show why and try again.
    private func ended(_ ended: Process) {
        guard ended === process, !stopping else { return }
        let message = String(decoding: errors, as: UTF8.self)
            .split(separator: "\n")
            .last
            .map { $0.replacingOccurrences(of: "✗ ", with: "") }
        problem = message ?? "Slingshot stopped unexpectedly."
        process = nil
        let wait = Self.backoff[min(restarts, Self.backoff.count - 1)]
        restarts += 1
        Task { @MainActor [weak self] in
            try? await Task.sleep(for: .seconds(wait))
            guard let self, !self.stopping, self.process == nil else { return }
            self.start()
        }
    }
}
