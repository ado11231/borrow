import AppKit
import SwiftUI

@main
struct SlingshotApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate

    var body: some Scene {
        MenuBarExtra {
            PopoverView(watcher: delegate.watcher)
        } label: {
            MenuBarLabel(watcher: delegate.watcher)
        }
        .menuBarExtraStyle(.window)
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    let watcher = Watcher()
    private let notifier = Notifier()

    func applicationDidFinishLaunching(_ notification: Notification) {
        notifier.setUp()
        watcher.onNotice = { [notifier] notice in notifier.post(notice) }
        watcher.start()
        LoginItem.enableOnFirstLaunch()
    }

    /// `slingshot menubar` opens the app again after saving a new program path or box.
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        watcher.start()
        return false
    }

    func applicationWillTerminate(_ notification: Notification) {
        watcher.stop()
    }
}

/// The part that is always visible: a symbol and the two numbers worth a glance.
struct MenuBarLabel: View {
    let watcher: Watcher

    var body: some View {
        if let status = watcher.status, status.online {
            HStack(spacing: 4) {
                Image(systemName: "server.rack")
                Text(summary(status)).monospacedDigit()
            }
        } else {
            HStack(spacing: 4) {
                Image(systemName: "server.rack")
                    .opacity(0.5)
                Text(watcher.status == nil && watcher.problem == nil ? "" : "offline")
            }
        }
    }

    private func summary(_ status: Status) -> String {
        [
            status.cpu.map { String(format: "%.0f%%", $0.percent) },
            status.memory.map(shortCapacity),
        ]
        .compactMap { $0 }
        .joined(separator: " · ")
    }
}
