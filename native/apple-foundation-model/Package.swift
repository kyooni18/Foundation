// swift-tools-version: 6.2

import PackageDescription

let package = Package(
    name: "FoundationAppleModelBridge",
    platforms: [
        .macOS(.v26)
    ],
    products: [
        .executable(
            name: "foundation-apple-bridge",
            targets: ["FoundationAppleBridge"]
        )
    ],
    targets: [
        .executableTarget(
            name: "FoundationAppleBridge"
        )
    ]
)
