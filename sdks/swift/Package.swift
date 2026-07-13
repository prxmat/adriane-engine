// swift-tools-version: 5.9

import PackageDescription

let package = Package(
    name: "Adriane",
    products: [
        .library(name: "Adriane", targets: ["Adriane"])
    ],
    targets: [
        .target(name: "CAdriane"),
        .target(name: "Adriane", dependencies: ["CAdriane"])
    ]
)
