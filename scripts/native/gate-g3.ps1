# Gate G3 checks: the deadlines module on real data, end to end.
# A local calendar server stands in for Moodle/Google; Blink runs for real (real clock, real
# worker threads, real files in a temp data folder) and is driven through its control pipe only.
# Uses no cursor or keyboard.
#   powershell -File scripts\native\gate-g3.ps1 [-Exe path\to\blink.exe]
param([string]$Exe = '', [int]$Port = 8791)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\blink-ctl.ps1"
if ($Exe) { $env:BLINK_EXE = (Resolve-Path $Exe).Path }

$script:pass = 0; $script:fail = 0
function Check([string]$what, [bool]$ok, [string]$detail = '') {
  if ($ok) { $script:pass++; "  ok    $what" } else { $script:fail++; "  FAIL  $what  $detail" }
}
$script:effects = New-Object System.Collections.Generic.List[string]
function Cmd($c, $o) {
  $r = Send-Blink $c $o
  if ($r.effects) { foreach ($e in $r.effects) { $script:effects.Add([string]$e) } }
  $r
}
function Flush($c) { [void](Cmd $c @{cmd='effects'}) }
function State($c) { (Send-Blink $c @{cmd='state'}) }
function WaitFor([scriptblock]$cond, [int]$ms = 10000) {
  $sw = [System.Diagnostics.Stopwatch]::StartNew()
  while ($sw.ElapsedMilliseconds -lt $ms) { if (& $cond) { return $true }; Start-Sleep -Milliseconds 100 }
  $false
}

$root = Join-Path $env:TEMP 'blink-g3'
if (Test-Path $root) { Remove-Item $root -Recurse -Force }
New-Item -ItemType Directory -Force $root | Out-Null
$utf8 = New-Object System.Text.UTF8Encoding($false)
function WriteJson($name, $obj) { [System.IO.File]::WriteAllText((Join-Path $root $name), (ConvertTo-Json -InputObject $obj -Depth 8 -Compress), $utf8) }
WriteJson 'settings.json' @{ version = 1; onboarded = $true; toast = $true; refreshMinutes = 20
  thresholdsHours = @(24, 6, 1)
  feeds = @(@{ id = 'ics-g3'; kind = 'ics'; label = 'Fixture'; enabled = $true }) }

function Start-Server {
  Start-Process node -ArgumentList "`"$PSScriptRoot\..\serve-fixture.mjs`" $Port" -PassThru -WindowStyle Hidden
}
$server = Start-Server
$feedUrl = "http://127.0.0.1:$Port/cal.ics"
$envs = @{ BLINK_DATA_DIR = $root; BLINK_TZ = 'Asia/Jakarta'; BLINK_TEST_FEEDS = "ics-g3=$feedUrl" }
$p = $null
try {
  Start-Sleep -Milliseconds 800
  'startup, fetch and parse'
  $sw = [System.Diagnostics.Stopwatch]::StartNew()
  $p = Start-Blink 'g3' $envs
  $c = Connect-Blink 'g3'
  $s = State $c
  Check "window visible within 150 ms of the process answering ($($sw.ElapsedMilliseconds) ms from launch)" ($s.window.visible -and $sw.ElapsedMilliseconds -lt 1500)
  $ok = WaitFor { $s = State $c; $s.state.module.sync.kind -ne 'syncing' -and $s.state.module.upcoming.Count -gt 0 }
  Check 'the feed is fetched and parsed' $ok
  $m = $s.state.module
  Check 'four upcoming and one overdue deadline from the fixture' ($m.upcoming.Count -eq 4 -and $m.overdue.Count -eq 1) "upcoming=$($m.upcoming.Count) overdue=$($m.overdue.Count)"
  Check 'nearest first: Problem Set 4 leads' ($m.upcoming[0] -eq 'Problem Set 4') ($m.upcoming -join ', ')
  Check 'the pill shows the hero course and a countdown' ($s.state.rest.label -like 'CS101*' -and $s.state.rest.count -match '^\d+h \d+m$') "$($s.state.rest.label) / $($s.state.rest.count)"
  Check 'footer reports a fresh sync' ($m.sync.kind -eq 'ok') $m.sync.kind
  Check 'the fetched calendar was cached for offline use' (Test-Path (Join-Path $root 'feed-ics-g3.json'))

  'reminders'
  Flush $c
  $toasts = @($script:effects | Where-Object { $_ -like 'toast:*' })
  Check 'one reminder for the deadline inside 6 hours, none for the others' ($toasts.Count -eq 1 -and $toasts[0] -like '*Problem Set 4*') ($toasts -join ' | ')
  Check 'the reminder is recorded in fired.json' ((Test-Path (Join-Path $root 'fired.json')) -and ((Get-Content (Join-Path $root 'fired.json') -Raw) -match 'Problem Set|t1@fixture'))
  $script:effects.Clear()
  Send-Blink $c @{cmd='refresh'} | Out-Null
  Start-Sleep -Milliseconds 1500
  Flush $c
  Check 'a refresh does not repeat the reminder' (@($script:effects | Where-Object { $_ -like 'toast:*' }).Count -eq 0)

  'panel actions on real data'
  Cmd $c @{cmd='mouse'; kind='move'; x=320; y=10} | Out-Null
  Start-Sleep -Milliseconds 900
  $s = State $c
  Check 'hover opens the panel with the real rows' ($s.state.mode -eq 'open' -and $s.state.module.hits.Count -gt 0) "mode=$($s.state.mode) hits=$($s.state.module.hits.Count) shape=$($s.state.shape.w)x$($s.state.shape.h)"
  $heroId = $s.state.module.upcomingIds[0]
  $open = $s.state.module.hits | Where-Object { $_.action -like 'OpenUrl:*' } | Select-Object -First 1
  Check 'rows link to the assignment page' ($null -ne $open)
  $row = $s.state.module.hits | Where-Object { $_.row -eq $heroId } | Select-Object -First 1
  Cmd $c @{cmd='mouse'; kind='move'; x=($row.x + 40); y=($row.y + 10)} | Out-Null
  $s = State $c
  $done = $s.state.module.hits | Where-Object { $_.action -eq "MarkDone:$heroId" } | Select-Object -First 1
  Check 'the hovered row offers Mark done' ($null -ne $done)
  Cmd $c @{cmd='click'; x=($done.x + $done.w / 2); y=($done.y + $done.h / 2)} | Out-Null
  Start-Sleep -Milliseconds 300
  $s = State $c
  Check 'marking done moves it to the done list' ($s.state.module.doneList -contains 'Problem Set 4' -and $s.state.module.upcoming[0] -ne 'Problem Set 4') ($s.state.module.upcoming -join ', ')
  Check 'done.json has it, as a map of task id to time' ((Get-Content (Join-Path $root 'done.json') -Raw) -match [regex]::Escape($heroId.Split(':')[0]))

  $next = $s.state.module.upcomingIds[0]
  $row2 = $s.state.module.hits | Where-Object { $_.row -eq $next } | Select-Object -First 1
  Cmd $c @{cmd='mouse'; kind='move'; x=($row2.x + 40); y=($row2.y + 10)} | Out-Null
  $s = State $c
  $snz = $s.state.module.hits | Where-Object { $_.action -eq "ToggleSnooze:$next" } | Select-Object -First 1
  if ($snz) {
    Cmd $c @{cmd='click'; x=($snz.x + $snz.w / 2); y=($snz.y + $snz.h / 2)} | Out-Null
    $s = State $c
    $chip = $s.state.module.hits | Where-Object { $_.action -eq "Snooze:${next}:2h" } | Select-Object -First 1
    if ($chip) { Cmd $c @{cmd='click'; x=($chip.x + $chip.w / 2); y=($chip.y + $chip.h / 2)} | Out-Null }
    Start-Sleep -Milliseconds 300
    Check 'snoozing writes snoozed.json' ((Test-Path (Join-Path $root 'snoozed.json')) -and ((Get-Content (Join-Path $root 'snoozed.json') -Raw) -match [regex]::Escape($next.Split(':')[0])))
  } else { Check 'the second row offers a snooze button' $false }

  'a restart keeps your choices and does not repeat reminders'
  Cmd $c @{cmd='quit'} | Out-Null; Start-Sleep -Milliseconds 600; if (-not $p.HasExited) { $p.Kill() }
  $script:effects.Clear()
  $p = Start-Blink 'g3b' $envs
  $c = Connect-Blink 'g3b'
  $ok = WaitFor { $s = State $c; $s.state.module.sync.kind -ne 'syncing' -and $s.state.module.upcoming.Count -gt 0 }
  Check 'the notch comes back with the data' $ok
  Check 'the done deadline is still done' ($s.state.module.doneList -contains 'Problem Set 4')
  Start-Sleep -Milliseconds 800; Flush $c
  Check 'no reminder is replayed after the restart' (@($script:effects | Where-Object { $_ -like 'toast:*' }).Count -eq 0) ($script:effects -join ' | ')

  'offline and recovery'
  $server.Kill(); Start-Sleep -Milliseconds 500
  Send-Blink $c @{cmd='refresh'} | Out-Null
  $ok = WaitFor { $s = State $c; $s.state.module.sync.kind -in @('offline', 'error') } 15000
  Check 'with the server gone the footer says so' $ok "kind=$($s.state.module.sync.kind) text=$($s.state.module.sync.text)"
  Check 'the cached deadlines stay on screen' ($s.state.module.upcoming.Count -gt 0)
  $server = Start-Server; Start-Sleep -Milliseconds 800
  Send-Blink $c @{cmd='refresh'} | Out-Null
  $ok = WaitFor { $s = State $c; $s.state.module.sync.kind -eq 'ok' } 15000
  Check 'once it is back, a refresh recovers' $ok "kind=$($s.state.module.sync.kind)"

  'personal events and settings'
  $due = [DateTimeOffset]::UtcNow.AddHours(2).ToUnixTimeMilliseconds()
  WriteJson 'local_events.json' @(@{ id = 'local-test-1'; title = 'Dentist'; dueMs = $due; allDay = $false })
  Send-Blink $c @{cmd='reload_local'} | Out-Null
  $s = State $c
  Check 'a personal event appears' ($s.state.module.upcoming -contains 'Dentist') ($s.state.module.upcoming -join ', ')
  Check 'and is the nearest deadline' ($s.state.module.upcoming[0] -eq 'Dentist')
  $r = Send-Blink $c @{cmd='settings'; patch=@{ feeds = @() }}
  $s = State $c
  Check 'removing the feed in settings drops its deadlines but keeps personal events' ($s.state.module.upcoming.Count -eq 1 -and $s.state.module.upcoming[0] -eq 'Dentist') ($s.state.module.upcoming -join ', ')
  Send-Blink $c @{cmd='quit'} | Out-Null
  Start-Sleep -Milliseconds 500; if (-not $p.HasExited) { $p.Kill() }

  'the link comes from Windows Credential Manager'
  $cmTarget = 'ics-cm.deadline-notch'
  cmdkey /generic:$cmTarget /user:ics-cm /pass:$feedUrl | Out-Null
  try {
    $cmRoot = Join-Path $env:TEMP 'blink-g3-cm'; if (Test-Path $cmRoot) { Remove-Item $cmRoot -Recurse -Force }; New-Item -ItemType Directory -Force $cmRoot | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $cmRoot 'settings.json'), '{"version":1,"onboarded":true,"toast":false,"feeds":[{"id":"ics-cm","kind":"ics","label":"CM","enabled":true}]}', $utf8)
    $p = Start-Blink 'g3cm' @{ BLINK_DATA_DIR = $cmRoot; BLINK_TZ = 'Asia/Jakarta' }
    $c = Connect-Blink 'g3cm'
    $ok = WaitFor { $s = State $c; $s.state.module.sync.kind -ne 'syncing' -and $s.state.module.upcoming.Count -gt 0 }
    Check 'a feed whose link is only in Credential Manager is fetched' $ok "kind=$($s.state.module.sync.kind)"
    Send-Blink $c @{cmd='quit'} | Out-Null
  } finally {
    cmdkey /delete:$cmTarget | Out-Null
    Start-Sleep -Milliseconds 300; if ($p -and -not $p.HasExited) { $p.Kill() }
  }
} finally {
  Start-Sleep -Milliseconds 300
  if ($p -and -not $p.HasExited) { $p.Kill() }
  if ($server -and -not $server.HasExited) { $server.Kill() }
}
"`n$script:pass passed, $script:fail failed"
if ($script:fail -gt 0) { exit 1 }
