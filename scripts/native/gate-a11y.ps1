# Accessibility checks with a real UI Automation client (the .NET one that screen readers'
# own stacks resemble): walks the notch's tree, reads names and patterns, invokes buttons, and
# listens for focus events. Needs no cursor or keyboard; the app is driven through its pipe.
#   powershell -File scripts\native\gate-a11y.ps1 [-Exe path\to\blink.exe]
param([string]$Exe = '')
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\blink-ctl.ps1"
if ($Exe) { $env:BLINK_EXE = (Resolve-Path $Exe).Path }
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type -TypeDefinition @'
using System; using System.Collections.Generic; using System.Runtime.InteropServices; using System.Windows.Automation;
public static class Win { [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t); }
public static class Focus {
  public static List<string> Names = new List<string>();
  public static void On(object s, AutomationFocusChangedEventArgs e) {
    try { var el = AutomationElement.FocusedElement; lock (Names) { Names.Add(el.Current.Name); } } catch { }
  }
  public static void Start() { Automation.AddAutomationFocusChangedEventHandler(new AutomationFocusChangedEventHandler(On)); }
  public static void Stop() { Automation.RemoveAllEventHandlers(); }
}
'@ -ReferencedAssemblies UIAutomationClient, UIAutomationTypes, WindowsBase

$script:pass = 0; $script:fail = 0
function Check([string]$what, [bool]$ok, [string]$detail = '') {
  if ($ok) { $script:pass++; "  ok    $what" } else { $script:fail++; "  FAIL  $what  $detail" }
}
function Kids($el) {
  $walker = [System.Windows.Automation.TreeWalker]::RawViewWalker
  $out = @(); $c = $walker.GetFirstChild($el)
  while ($c) { $out += $c; $c = $walker.GetNextSibling($c) }
  $out
}
function ElName($el) { $el.Current.Name }
function ElType($el) { $el.Current.ControlType.ProgrammaticName -replace 'ControlType\.', '' }
function ElFind($el, [string]$like) { Kids $el | Where-Object { (ElName $_) -like $like } | Select-Object -First 1 }
function Invoke-El($el) { ([System.Windows.Automation.InvokePattern]$el.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)).Invoke() }

$data = Join-Path $env:TEMP 'blink-a11y-data'; New-Item -ItemType Directory -Force $data | Out-Null
$p = Start-Blink 'a11y' @{ BLINK_MANUAL_CLOCK = '1'; BLINK_DATA_DIR = $data; BLINK_TZ = 'Asia/Jakarta'; BLINK_DEMO = '1' }
try {
  $c = Connect-Blink 'a11y'
  function Ctl($o) { Send-Blink $c $o }
  $s = Ctl @{cmd='state'}

  'the notch at rest'
  $hwnd = [IntPtr][int64]$s.window.hwnd   # ours, not whichever notch Windows finds first
  $root = [System.Windows.Automation.AutomationElement]::FromHandle($hwnd)
  Check 'the window is known to UI Automation as "Blink"' ((ElName $root) -eq 'Blink') (ElName $root)
  Check 'it is a pane with its own class name' ((ElType $root) -eq 'Pane' -and $root.Current.ClassName -eq 'BlinkNotch') "$(ElType $root) $($root.Current.ClassName)"
  $kids = Kids $root
  Check 'at rest it has one child: the pill, as a button' ($kids.Count -eq 1 -and (ElType $kids[0]) -eq 'Button') "$($kids.Count) children"
  Check 'the pill reads like a sentence a person can follow' ((ElName $kids[0]) -match '^Next deadline: CS101.* \d+h \d+m$') (ElName $kids[0])
  $r = $kids[0].Current.BoundingRectangle
  $wr = $s.window.rect
  Check 'its rectangle lies inside the window on screen' ($r.X -ge $wr.x -and $r.Y -ge $wr.y -and $r.Right -le ($wr.x + $wr.w) -and $r.Height -gt 0) "$r vs $($wr.x),$($wr.y)"
  $hit = [System.Windows.Automation.AutomationElement]::FromPoint((New-Object System.Windows.Point(($r.X + $r.Width / 2), ($r.Y + $r.Height / 2))))
  Check 'asking what is at the pill''s centre finds the pill' ($hit -and (ElName $hit) -match '^(Next deadline|Clock)') (ElName $hit)

  'opening it through the screen reader'
  [System.Windows.Automation.Automation] | Out-Null
  [Focus]::Start()
  Invoke-El $kids[0]
  Start-Sleep -Milliseconds 400
  $s = Ctl @{cmd='state'}
  Check 'invoking the pill opens the panel in keyboard mode' ($s.state.mode -eq 'open' -and $s.state.keyboard) "$($s.state.mode) $($s.state.keyboard)"
  Ctl @{cmd='advance'; ms=700} | Out-Null
  Start-Sleep -Milliseconds 300
  $kids = Kids $root
  $names = $kids | ForEach-Object { ElName $_ }
  Check 'the panel has a list of upcoming deadlines, an overdue list and footer buttons' (($names -contains 'Upcoming deadlines') -and ($names -contains 'Overdue') -and ($names -contains 'Refresh now') -and ($names -contains 'Settings')) ($names -join ' | ')
  $list = ElFind $root 'Upcoming deadlines'
  Check 'the list is a list' ((ElType $list) -eq 'List')
  $rows = Kids $list
  Check 'it has one item per deadline' ($rows.Count -eq 5) "$($rows.Count)"
  Check 'each item says title, course, when and how long' ((ElName $rows[0]) -match '^Problem Set 4\. CS101 Algorithms\. .+\. \d+h \d+m$') (ElName $rows[0])
  Check 'items explain the keys in their help text' ($rows[0].Current.HelpText -match 'Enter') $rows[0].Current.HelpText
  $itemKids = Kids $rows[0]
  $buttonNames = $itemKids | ForEach-Object { ElName $_ }
  Check 'each deadline has Mark done and three snooze buttons' (($buttonNames -join '|') -match 'Mark done: Problem Set 4\|Snooze 30 minutes.*\|Snooze 2 hours.*\|Snooze until tomorrow') ($buttonNames -join ' | ')
  Check 'the selected row has keyboard focus' ($rows[0].Current.HasKeyboardFocus)
  Check 'the others do not' (-not $rows[1].Current.HasKeyboardFocus)
  Check 'a deadline with a page can be invoked' ($rows[0].GetSupportedPatterns().Count -ge 1)

  'moving the selection announces focus changes'
  Ctl @{cmd='key'; key='down'} | Out-Null
  Start-Sleep -Milliseconds 500
  $seen = [Focus]::Names.ToArray()
  # The event names come from asking Windows who has focus. With focus simulated that is another
  # app, so only the arrival of the event can be checked here (real focus is in real-input.ps1).
  Check 'moving the selection raises a focus-changed event' ($seen.Count -ge 1) "events=$($seen.Count)"
  $rows = Kids (ElFind $root 'Upcoming deadlines')
  Check 'and the newly selected row is the one reporting keyboard focus' ((-not $rows[0].Current.HasKeyboardFocus) -and $rows[1].Current.HasKeyboardFocus) "$($rows[0].Current.HasKeyboardFocus) $($rows[1].Current.HasKeyboardFocus)"

  'acting through the screen reader'
  $list = ElFind $root 'Upcoming deadlines'; $rows = Kids $list
  $first = $rows[0]; $firstName = ElName $first
  $done = ElFind $first 'Mark done*'
  Ctl @{cmd='effects'} | Out-Null
  Invoke-El $done
  Start-Sleep -Milliseconds 400
  $s = Ctl @{cmd='state'}
  Check 'invoking "Mark done" marks it done in the app' ($s.state.module.doneList -contains 'Problem Set 4') ($s.state.module.doneList -join ',')
  $fx = (Ctl @{cmd='effects'}).effects
  Check 'and the app says so for the screen reader' (@($fx | Where-Object { $_ -like 'announce:Marked done: Problem Set 4' }).Count -eq 1) ($fx -join ' | ')
  Start-Sleep -Milliseconds 300
  $list = ElFind $root 'Upcoming deadlines'; $rows = Kids $list
  Check 'the list no longer contains it' ($rows.Count -eq 4 -and (ElName $rows[0]) -notlike 'Problem Set 4*') (ElName $rows[0])
  $doneToggle = ElFind $root 'Show done*'
  Check 'a "Show done (1)" button appears' ($null -ne $doneToggle -and (ElName $doneToggle) -match '\(1\)') (ElName $doneToggle)
  Invoke-El $doneToggle
  Start-Sleep -Milliseconds 300
  $doneList = ElFind $root 'Done'
  Check 'invoking it reveals the done list with a Restore button' ($doneList -and ((Kids (Kids $doneList)[0] | ForEach-Object { ElName $_ }) -join '') -like 'Restore*')
  Invoke-El (ElFind (Kids $doneList)[0] 'Restore*')
  Start-Sleep -Milliseconds 300
  $s = Ctl @{cmd='state'}
  Check 'Restore brings it back' ($s.state.module.doneList.Count -eq 0 -and $s.state.module.upcoming -contains 'Problem Set 4')
  $second = (Kids (ElFind $root 'Upcoming deadlines'))[1]
  Invoke-El (ElFind $second 'Snooze 2 hours*')
  Start-Sleep -Milliseconds 300
  $fx = (Ctl @{cmd='effects'}).effects
  Check 'snoozing is announced too' (@($fx | Where-Object { $_ -like 'announce:Snoozed:*' }).Count -eq 1) ($fx -join ' | ')

  'closing it again'
  Ctl @{cmd='key'; key='escape'} | Out-Null
  Ctl @{cmd='advance'; ms=700} | Out-Null
  Start-Sleep -Milliseconds 300
  $kids = Kids $root
  Check 'the tree returns to the single pill' ($kids.Count -eq 1 -and (ElName $kids[0]) -like 'Next deadline*') "$($kids.Count)"

  'language'
  Ctl @{cmd='settings'; patch=@{language='id'}} | Out-Null
  Start-Sleep -Milliseconds 400
  $kids = Kids $root
  Check 'the names follow the language setting' ((ElName $kids[0]) -like 'Tenggat berikutnya*') (ElName $kids[0])
  [Focus]::Stop()
  Ctl @{cmd='quit'} | Out-Null
} finally { Start-Sleep -Milliseconds 300; if (-not $p.HasExited) { $p.Kill() } }
"`n$script:pass passed, $script:fail failed"
if ($script:fail -gt 0) { exit 1 }
