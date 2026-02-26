#!/usr/bin/env swift
// CoreWLAN Wi-Fi scanner — called by Overwatch SIGINT collector
// Output: one line per network: SSID|BSSID|RSSI|CHANNEL|BAND
// Privacy: SSID and BSSID are intentionally included here (Mode A stripping
// happens in the Rust collector before any data leaves the node)

import CoreWLAN
import Foundation

let client = CWWiFiClient.shared()
guard let iface = client.interface() else {
    fputs("ERROR: no Wi-Fi interface\n", stderr)
    exit(1)
}

do {
    let networks = try iface.scanForNetworks(withSSID: nil)
    for net in networks {
        let ssid = net.ssid ?? ""
        let bssid = net.bssid ?? ""
        let rssi = net.rssiValue
        let ch = net.wlanChannel
        let channel = ch?.channelNumber ?? 0
        let band: String
        switch ch?.channelBand {
        case .band2GHz: band = "2.4"
        case .band5GHz: band = "5"
        case .band6GHz: band = "6"
        default: band = channel > 14 ? "5" : "2.4"
        }
        print("\(ssid)|\(bssid)|\(rssi)|\(channel)|\(band)")
    }
} catch {
    fputs("ERROR: \(error)\n", stderr)
    exit(1)
}
