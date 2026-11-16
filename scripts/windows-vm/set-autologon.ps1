# Switch the account Windows logs on automatically (e.g. to test as the Japanese-named user), as admin over SSH:
#   powershell -File C:\ProgramData\yhtye-vm\set-autologon.ps1 -User NAME -Password PW; Restart-Computer -Force
# The password goes into the LSA secret DefaultPassword (where autounattend.xml's AutoLogon put it, and where
# Winlogon reads it first), as Sysinternals Autologon does. Run it again with the admin user to switch back.
# A non-ASCII name can be passed from a UTF-8 .ps1 with a BOM; this file itself is ASCII (Windows PowerShell 5.1
# reads BOM-less scripts in the ANSI code page).
param([Parameter(Mandatory=$true)][string]$User, [Parameter(Mandatory=$true)][string]$Password)
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class YhtyeLsa {
  [StructLayout(LayoutKind.Sequential)] public struct LSA_UNICODE_STRING { public ushort Length; public ushort MaximumLength; public IntPtr Buffer; }
  [StructLayout(LayoutKind.Sequential)] public struct LSA_OBJECT_ATTRIBUTES { public int Length; public IntPtr RootDirectory; public IntPtr ObjectName; public uint Attributes; public IntPtr SecurityDescriptor; public IntPtr SecurityQualityOfService; }
  [DllImport("advapi32.dll")] static extern uint LsaOpenPolicy(IntPtr SystemName, ref LSA_OBJECT_ATTRIBUTES ObjectAttributes, uint DesiredAccess, out IntPtr PolicyHandle);
  [DllImport("advapi32.dll")] static extern uint LsaStorePrivateData(IntPtr PolicyHandle, ref LSA_UNICODE_STRING KeyName, ref LSA_UNICODE_STRING PrivateData);
  [DllImport("advapi32.dll")] static extern uint LsaClose(IntPtr h);
  [DllImport("advapi32.dll")] static extern uint LsaNtStatusToWinError(uint s);
  static LSA_UNICODE_STRING S(string s) { var u = new LSA_UNICODE_STRING(); u.Buffer = Marshal.StringToHGlobalUni(s); u.Length = (ushort)(s.Length * 2); u.MaximumLength = (ushort)(s.Length * 2 + 2); return u; }
  public static void Store(string key, string value) {
    var oa = new LSA_OBJECT_ATTRIBUTES(); IntPtr h;
    uint r = LsaOpenPolicy(IntPtr.Zero, ref oa, 0x00000020 | 0x00000004, out h); // POLICY_CREATE_SECRET | POLICY_GET_PRIVATE_INFORMATION
    if (r != 0) throw new Exception("LsaOpenPolicy " + LsaNtStatusToWinError(r));
    var k = S(key); var v = S(value);
    r = LsaStorePrivateData(h, ref k, ref v); LsaClose(h);
    if (r != 0) throw new Exception("LsaStorePrivateData " + LsaNtStatusToWinError(r));
  }
}
'@
[YhtyeLsa]::Store('DefaultPassword', $Password)
$k = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon'
Set-ItemProperty $k -Name AutoAdminLogon -Value '1'
Set-ItemProperty $k -Name DefaultUserName -Value $User
Set-ItemProperty $k -Name DefaultDomainName -Value $env:COMPUTERNAME
"autologon -> $User"
