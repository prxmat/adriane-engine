import Foundation
import CAdriane

public struct AdrianeError: Error, CustomStringConvertible {
    public let code: Int32
    public let message: String

    public var description: String { message }
}

public enum Adriane {
    public static func engineVersion() -> String {
        guard let ptr = adriane_engine_version() else { return "" }
        defer { adriane_string_free(ptr) }
        return String(cString: ptr)
    }

    public static func validateGraphJson(_ definitionJson: String) throws -> String {
        let result = definitionJson.withCString { adriane_validate_graph_json($0) }
        return try unwrap(result)
    }

    public static func compileGraphYamlJson(_ yaml: String) throws -> String {
        let result = yaml.withCString { adriane_compile_graph_yaml_json($0) }
        return try unwrap(result)
    }

    public static func availableProvidersJson() throws -> String {
        try unwrap(adriane_available_providers_json())
    }

    public static func resolveModelJson(_ tier: String, availableJson: String? = nil, overrideJson: String? = nil) throws -> String {
        let result = tier.withCString { tierPtr in
            withOptionalCString(availableJson) { availablePtr in
                withOptionalCString(overrideJson) { overridePtr in
                    adriane_resolve_model_json(tierPtr, availablePtr, overridePtr)
                }
            }
        }
        return try unwrap(result)
    }

    public static func listComponentsJson() throws -> String {
        try unwrap(adriane_list_components_json())
    }

    public static func listPrebuiltJson() throws -> String {
        try unwrap(adriane_list_prebuilt_json())
    }

    public static func runComponentJson(kind: String, paramsJson: String, channelsJson: String) throws -> String {
        let result = kind.withCString { kindPtr in
            paramsJson.withCString { paramsPtr in
                channelsJson.withCString { channelsPtr in
                    adriane_run_component_json(kindPtr, paramsPtr, channelsPtr)
                }
            }
        }
        return try unwrap(result)
    }

    public static func runPrebuiltJson(name: String, inputJson: String, optionsJson: String? = nil) throws -> String {
        let result = name.withCString { namePtr in
            inputJson.withCString { inputPtr in
                withOptionalCString(optionsJson) { optionsPtr in
                    adriane_run_prebuilt_json(namePtr, inputPtr, optionsPtr)
                }
            }
        }
        return try unwrap(result)
    }

    private static func unwrap(_ result: AdrianeResult) throws -> String {
        defer { adriane_result_free(result) }
        if result.code == 0 {
            guard let value = result.value else { return "" }
            return String(cString: value)
        }

        let message = result.error.map { String(cString: $0) } ?? "Adriane C API error \(result.code)"
        throw AdrianeError(code: result.code, message: message)
    }

    private static func withOptionalCString<T>(_ value: String?, _ body: (UnsafePointer<CChar>?) throws -> T) rethrows -> T {
        guard let value else {
            return try body(nil)
        }
        return try value.withCString(body)
    }
}
