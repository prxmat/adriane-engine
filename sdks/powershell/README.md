# Adriane PowerShell SDK

PowerShell module over `adriane-c-api` using an embedded C# P/Invoke bridge.

```powershell
Import-Module ./Adriane.psm1
Get-AdrianeEngineVersion
Get-AdrianeComponentsJson
```

The native `adriane_c_api` library must be on the platform loader path.
