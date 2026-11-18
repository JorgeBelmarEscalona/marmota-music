// swift-tools-version:5.9
// Ayudante sin ventana que reproduce Apple Music con MusicKit para el notch.
import PackageDescription

let package = Package(
    name: "MarmotaAppleMusic",
    platforms: [.macOS(.v14)],
    targets: [
        .executableTarget(name: "MarmotaAppleMusic", path: "Sources/MarmotaAppleMusic"),
    ]
)
