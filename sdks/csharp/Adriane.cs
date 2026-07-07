using System;
using System.Runtime.InteropServices;

namespace Adriane;

public sealed class AdrianeException : Exception
{
    public int Code { get; }

    public AdrianeException(int code, string message) : base(message)
    {
        Code = code;
    }
}

public static class Adriane
{
    [StructLayout(LayoutKind.Sequential)]
    private struct AdrianeResult
    {
        public int Code;
        public IntPtr Value;
        public IntPtr Error;
    }

    private const string Library = "adriane_c_api";

    static Adriane()
    {
        var path = Environment.GetEnvironmentVariable("ADRIANE_C_API_LIB");
        if (string.IsNullOrWhiteSpace(path))
        {
            return;
        }

        NativeLibrary.SetDllImportResolver(typeof(Adriane).Assembly, (libraryName, assembly, searchPath) =>
        {
            return libraryName == Library ? NativeLibrary.Load(path) : IntPtr.Zero;
        });
    }

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr adriane_engine_version();

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern AdrianeResult adriane_validate_graph_json(string definitionJson);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern AdrianeResult adriane_compile_graph_yaml_json(string yaml);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern AdrianeResult adriane_available_providers_json();

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern AdrianeResult adriane_resolve_model_json(string tier, string? availableJson, string? overrideJson);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern AdrianeResult adriane_list_components_json();

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern AdrianeResult adriane_list_prebuilt_json();

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern AdrianeResult adriane_run_component_json(string kind, string paramsJson, string channelsJson);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern AdrianeResult adriane_run_prebuilt_json(string name, string inputJson, string? optionsJson);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern void adriane_string_free(IntPtr ptr);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern void adriane_result_free(AdrianeResult result);

    public static string EngineVersion()
    {
        var ptr = adriane_engine_version();
        if (ptr == IntPtr.Zero)
        {
            return string.Empty;
        }

        try
        {
            return Marshal.PtrToStringUTF8(ptr) ?? string.Empty;
        }
        finally
        {
            adriane_string_free(ptr);
        }
    }

    public static string ValidateGraphJson(string definitionJson) => Unwrap(adriane_validate_graph_json(definitionJson));

    public static string CompileGraphYamlJson(string yaml) => Unwrap(adriane_compile_graph_yaml_json(yaml));

    public static string AvailableProvidersJson() => Unwrap(adriane_available_providers_json());

    public static string ResolveModelJson(string tier, string? availableJson = null, string? overrideJson = null) =>
        Unwrap(adriane_resolve_model_json(tier, availableJson, overrideJson));

    public static string ListComponentsJson() => Unwrap(adriane_list_components_json());

    public static string ListPrebuiltJson() => Unwrap(adriane_list_prebuilt_json());

    public static string RunComponentJson(string kind, string paramsJson, string channelsJson) =>
        Unwrap(adriane_run_component_json(kind, paramsJson, channelsJson));

    public static string RunPrebuiltJson(string name, string inputJson, string? optionsJson = null) =>
        Unwrap(adriane_run_prebuilt_json(name, inputJson, optionsJson));

    private static string Unwrap(AdrianeResult result)
    {
        try
        {
            if (result.Code == 0)
            {
                return Marshal.PtrToStringUTF8(result.Value) ?? string.Empty;
            }

            var message = result.Error == IntPtr.Zero
                ? $"Adriane C API error {result.Code}"
                : Marshal.PtrToStringUTF8(result.Error) ?? $"Adriane C API error {result.Code}";
            throw new AdrianeException(result.Code, message);
        }
        finally
        {
            adriane_result_free(result);
        }
    }
}
