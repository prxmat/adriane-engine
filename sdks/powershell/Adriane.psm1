$NativeLibrary = if ($env:ADRIANE_C_API_LIB) {
  $env:ADRIANE_C_API_LIB.Replace("\", "\\")
} else {
  "adriane_c_api"
}

$Source = @"
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class AdrianeNative {
  [StructLayout(LayoutKind.Sequential)]
  public struct AdrianeResult {
    public int Code;
    public IntPtr Value;
    public IntPtr Error;
  }

  [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
  public delegate int AdrianeStringCallback(
    IntPtr payloadJson,
    IntPtr userData,
    out IntPtr value,
    out IntPtr error);

  [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
  public delegate void AdrianeEventCallback(IntPtr payloadJson, IntPtr userData);

  [StructLayout(LayoutKind.Sequential)]
  public struct AdrianeCallbacks {
    public IntPtr UserData;
    public AdrianeStringCallback OnNode;
    public AdrianeStringCallback OnCondition;
    public AdrianeEventCallback OnEvent;
  }

  public static string PtrToUtf8(IntPtr ptr) {
    if (ptr == IntPtr.Zero) {
      return "";
    }

    int len = 0;
    while (Marshal.ReadByte(ptr, len) != 0) {
      len++;
    }

    byte[] bytes = new byte[len];
    Marshal.Copy(ptr, bytes, 0, len);
    return Encoding.UTF8.GetString(bytes);
  }

  [DllImport("$NativeLibrary", CallingConvention = CallingConvention.Cdecl)]
  public static extern IntPtr adriane_engine_version();
  [DllImport("$NativeLibrary", CallingConvention = CallingConvention.Cdecl)]
  public static extern AdrianeResult adriane_validate_graph_json(string definitionJson);
  [DllImport("$NativeLibrary", CallingConvention = CallingConvention.Cdecl)]
  public static extern AdrianeResult adriane_compile_graph_yaml_json(string yaml);
  [DllImport("$NativeLibrary", CallingConvention = CallingConvention.Cdecl)]
  public static extern AdrianeResult adriane_available_providers_json();
  [DllImport("$NativeLibrary", CallingConvention = CallingConvention.Cdecl)]
  public static extern AdrianeResult adriane_resolve_model_json(string tier, string availableJson, string overrideJson);
  [DllImport("$NativeLibrary", CallingConvention = CallingConvention.Cdecl)]
  public static extern AdrianeResult adriane_list_components_json();
  [DllImport("$NativeLibrary", CallingConvention = CallingConvention.Cdecl)]
  public static extern AdrianeResult adriane_list_prebuilt_json();
  [DllImport("$NativeLibrary", CallingConvention = CallingConvention.Cdecl)]
  public static extern AdrianeResult adriane_run_component_json(string kind, string paramsJson, string channelsJson);
  [DllImport("$NativeLibrary", CallingConvention = CallingConvention.Cdecl)]
  public static extern AdrianeResult adriane_run_prebuilt_json(string name, string inputJson, string optionsJson);
  [DllImport("$NativeLibrary", CallingConvention = CallingConvention.Cdecl)]
  public static extern AdrianeResult adriane_engine_run_json(string specJson, AdrianeCallbacks callbacks);
  [DllImport("$NativeLibrary", CallingConvention = CallingConvention.Cdecl)]
  public static extern AdrianeResult adriane_engine_resume_json(string specJson, AdrianeCallbacks callbacks);
  [DllImport("$NativeLibrary", CallingConvention = CallingConvention.Cdecl)]
  public static extern AdrianeResult adriane_engine_approve_and_resume_json(string specJson, AdrianeCallbacks callbacks);
  [DllImport("$NativeLibrary", CallingConvention = CallingConvention.Cdecl)]
  public static extern AdrianeResult adriane_engine_signal_json(string specJson, string signalName, string payloadJson, AdrianeCallbacks callbacks);
  [DllImport("$NativeLibrary", CallingConvention = CallingConvention.Cdecl)]
  public static extern AdrianeResult adriane_engine_replay_json(string specJson, string checkpointId, AdrianeCallbacks callbacks);
  [DllImport("$NativeLibrary", CallingConvention = CallingConvention.Cdecl)]
  public static extern void adriane_string_free(IntPtr ptr);
  [DllImport("$NativeLibrary", CallingConvention = CallingConvention.Cdecl)]
  public static extern void adriane_result_free(AdrianeResult result);
}
"@

if (-not ("AdrianeNative" -as [type])) {
  Add-Type -TypeDefinition $Source
}

function ConvertFrom-AdrianeResult {
  param([AdrianeNative+AdrianeResult] $Result)
  try {
    if ($Result.Code -eq 0) {
      return [AdrianeNative]::PtrToUtf8($Result.Value)
    }
    $message = if ($Result.Error -eq [IntPtr]::Zero) {
      "Adriane C API error $($Result.Code)"
    } else {
      [AdrianeNative]::PtrToUtf8($Result.Error)
    }
    throw $message
  } finally {
    [AdrianeNative]::adriane_result_free($Result)
  }
}

function Get-AdrianeEngineVersion {
  $ptr = [AdrianeNative]::adriane_engine_version()
  try {
    [AdrianeNative]::PtrToUtf8($ptr)
  } finally {
    if ($ptr -ne [IntPtr]::Zero) {
      [AdrianeNative]::adriane_string_free($ptr)
    }
  }
}

function Test-AdrianeGraphJson {
  param([string] $DefinitionJson)
  ConvertFrom-AdrianeResult ([AdrianeNative]::adriane_validate_graph_json($DefinitionJson))
}

function ConvertFrom-AdrianeGraphYaml {
  param([string] $Yaml)
  ConvertFrom-AdrianeResult ([AdrianeNative]::adriane_compile_graph_yaml_json($Yaml))
}

function Get-AdrianeAvailableProvidersJson {
  ConvertFrom-AdrianeResult ([AdrianeNative]::adriane_available_providers_json())
}

function Resolve-AdrianeModelJson {
  param([string] $Tier, [string] $AvailableJson = $null, [string] $OverrideJson = $null)
  ConvertFrom-AdrianeResult ([AdrianeNative]::adriane_resolve_model_json($Tier, $AvailableJson, $OverrideJson))
}

function Get-AdrianeComponentsJson {
  ConvertFrom-AdrianeResult ([AdrianeNative]::adriane_list_components_json())
}

function Get-AdrianePrebuiltJson {
  ConvertFrom-AdrianeResult ([AdrianeNative]::adriane_list_prebuilt_json())
}

function Invoke-AdrianeComponentJson {
  param([string] $Kind, [string] $ParamsJson, [string] $ChannelsJson)
  ConvertFrom-AdrianeResult ([AdrianeNative]::adriane_run_component_json($Kind, $ParamsJson, $ChannelsJson))
}

function Invoke-AdrianePrebuiltJson {
  param([string] $Name, [string] $InputJson, [string] $OptionsJson = $null)
  ConvertFrom-AdrianeResult ([AdrianeNative]::adriane_run_prebuilt_json($Name, $InputJson, $OptionsJson))
}

function Invoke-AdrianeEngineRunJson {
  param([string] $SpecJson, [AdrianeNative+AdrianeCallbacks] $Callbacks)
  ConvertFrom-AdrianeResult ([AdrianeNative]::adriane_engine_run_json($SpecJson, $Callbacks))
}

function Invoke-AdrianeEngineResumeJson {
  param([string] $SpecJson, [AdrianeNative+AdrianeCallbacks] $Callbacks)
  ConvertFrom-AdrianeResult ([AdrianeNative]::adriane_engine_resume_json($SpecJson, $Callbacks))
}

function Invoke-AdrianeEngineApproveAndResumeJson {
  param([string] $SpecJson, [AdrianeNative+AdrianeCallbacks] $Callbacks)
  ConvertFrom-AdrianeResult ([AdrianeNative]::adriane_engine_approve_and_resume_json($SpecJson, $Callbacks))
}

function Invoke-AdrianeEngineSignalJson {
  param([string] $SpecJson, [string] $SignalName, [string] $PayloadJson, [AdrianeNative+AdrianeCallbacks] $Callbacks)
  ConvertFrom-AdrianeResult ([AdrianeNative]::adriane_engine_signal_json($SpecJson, $SignalName, $PayloadJson, $Callbacks))
}

function Invoke-AdrianeEngineReplayJson {
  param([string] $SpecJson, [string] $CheckpointId, [AdrianeNative+AdrianeCallbacks] $Callbacks)
  ConvertFrom-AdrianeResult ([AdrianeNative]::adriane_engine_replay_json($SpecJson, $CheckpointId, $Callbacks))
}

Export-ModuleMember -Function `
  Get-AdrianeEngineVersion, `
  Test-AdrianeGraphJson, `
  ConvertFrom-AdrianeGraphYaml, `
  Get-AdrianeAvailableProvidersJson, `
  Resolve-AdrianeModelJson, `
  Get-AdrianeComponentsJson, `
  Get-AdrianePrebuiltJson, `
  Invoke-AdrianeComponentJson, `
  Invoke-AdrianePrebuiltJson, `
  Invoke-AdrianeEngineRunJson, `
  Invoke-AdrianeEngineResumeJson, `
  Invoke-AdrianeEngineApproveAndResumeJson, `
  Invoke-AdrianeEngineSignalJson, `
  Invoke-AdrianeEngineReplayJson
