import Foundation
import ServiceManagement

/// Starting at login is what makes installing once enough. The app turns it on the first
/// time it runs, and the toggle in the panel turns it off.
enum LoginItem {
    private static let offeredKey = "loginItemOffered"

    static var enabled: Bool {
        SMAppService.mainApp.status == .enabled
    }

    static func enableOnFirstLaunch() {
        let defaults = UserDefaults.standard
        guard !defaults.bool(forKey: offeredKey) else { return }
        defaults.set(true, forKey: offeredKey)
        set(true)
    }

    static func set(_ on: Bool) {
        do {
            if on {
                try SMAppService.mainApp.register()
            } else {
                try SMAppService.mainApp.unregister()
            }
        } catch {
            NSLog("Slingshot could not change the login item: \(error.localizedDescription)")
        }
    }
}
