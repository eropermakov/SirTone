// «Close the window» for the real-app harnesses: WM_CLOSE to this test's own main window only — the
// largest visible «Tauri Window» of the process (the widget is the small one). Not
// `Process.CloseMainWindow()`: with the window in the tray .NET takes a hidden helper window for
// the main one (the single-instance lock window, D-124), and the close would go there instead.

/** A PowerShell script that closes the main window of process `pid`; prints `closed` or `none`. */
export const closeMainScript = (pid) => `
Add-Type -Namespace SirtoneE2e -Name W -MemberDefinition @'
public delegate bool EnumProc(System.IntPtr h, System.IntPtr l);
[System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)] public struct R { public int L, T, Rt, B; }
[DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, System.IntPtr l);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(System.IntPtr h, out uint pid);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(System.IntPtr h);
[DllImport("user32.dll")] public static extern bool GetWindowRect(System.IntPtr h, out R r);
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(System.IntPtr h, System.Text.StringBuilder s, int n);
[DllImport("user32.dll")] public static extern bool PostMessageW(System.IntPtr h, uint m, System.IntPtr w, System.IntPtr l);
'@
$best = [IntPtr]::Zero; $area = 0
[SirtoneE2e.W]::EnumWindows({ param($h, $l)
  $p = 0; [void][SirtoneE2e.W]::GetWindowThreadProcessId($h, [ref]$p)
  if ($p -eq ${pid} -and [SirtoneE2e.W]::IsWindowVisible($h)) {
    $c = New-Object System.Text.StringBuilder 64; [void][SirtoneE2e.W]::GetClassNameW($h, $c, 64)
    $r = New-Object SirtoneE2e.W+R; [void][SirtoneE2e.W]::GetWindowRect($h, [ref]$r)
    $a = ($r.Rt - $r.L) * ($r.B - $r.T)
    if ($c.ToString() -eq 'Tauri Window' -and $a -gt $script:area) { $script:best = $h; $script:area = $a }
  }
  $true }, [IntPtr]::Zero) | Out-Null
if ($best -ne [IntPtr]::Zero) { [void][SirtoneE2e.W]::PostMessageW($best, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero); 'closed' } else { 'none' }
`;
