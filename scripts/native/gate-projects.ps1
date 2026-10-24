# The projects page through the real window: tasks typed with the keyboard commands, statuses,
# the saved file, files dropped (the drop command stands in for dragging from Explorer), opening
# and removing them. No cursor, no keyboard.
#   powershell -File scripts\native\gate-projects.ps1 [-Exe path\to\blink.exe]
param([string]$Exe = '')
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\blink-ctl.ps1"
if ($Exe) { $env:BLINK_EXE = (Resolve-Path $Exe).Path }
Remove-Item Env:BLINK_REAL_SOURCES -ErrorAction SilentlyContinue
$script:pass = 0; $script:fail = 0
function Check([string]$what, [bool]$ok, [string]$detail = '') { if ($ok) { $script:pass++; "  ok    $what" } else { $script:fail++; "  FAIL  $what  $detail" } }
$data = Join-Path $env:TEMP 'blink-projects-data'; if (Test-Path $data) { Remove-Item $data -Recurse -Force }; New-Item -ItemType Directory -Force $data | Out-Null
$files = Join-Path $env:TEMP 'blink-projects-files'; if (Test-Path $files) { Remove-Item $files -Recurse -Force }; New-Item -ItemType Directory -Force (Join-Path $files 'repo') | Out-Null; Set-Content (Join-Path $files 'spec.pdf') 'x'
$p = Start-Blink 'projects' @{ BLINK_MANUAL_CLOCK = '1'; BLINK_DATA_DIR = $data; BLINK_TZ = 'Asia/Jakarta'; BLINK_DEMO = '1' }
try {
  $c = Connect-Blink 'projects'
  function Ctl($o) { Send-Blink $c $o }
  function Mod() { (Ctl @{cmd='state'}).state.module }
  function Hit($pattern) { (Mod).hits | Where-Object { $_.target -like $pattern } | Select-Object -First 1 }
  $script:log = @()
  function ClickAt($h) { $x = $h.x + $h.w / 2; $y = $h.y + $h.h / 2; Ctl @{cmd='mouse'; kind='move'; x=$x; y=$y} | Out-Null; $d = Ctl @{cmd='mouse'; kind='down'; x=$x; y=$y}; $u = Ctl @{cmd='mouse'; kind='up'; x=$x; y=$y}; $script:log += @($d.effects) + @($u.effects); Ctl @{cmd='advance'; ms=60} | Out-Null }
  function TypeLine($text) { foreach ($ch in $text.ToCharArray()) { if ($ch -eq ' ') { Ctl @{cmd='key'; key='space'} | Out-Null } else { Ctl @{cmd='key'; key='char'; ch=[string]$ch} | Out-Null } }; Ctl @{cmd='key'; key='enter'} | Out-Null; Ctl @{cmd='advance'; ms=60} | Out-Null }

  $m = Ctl @{cmd='modules'}
  Check 'projects is one of the pages (after music)' (($m.modules | ForEach-Object { $_.id }) -join ',' -eq 'clock,deadlines,controls,music,projects,agents,system') (($m.modules | ForEach-Object { $_.id }) -join ',')
  Check 'the window is a drop target' ((Ctl @{cmd='state'}).window.sources.dropTarget -eq $true)
  Ctl @{cmd='mouse'; kind='move'; x=320; y=10} | Out-Null; Ctl @{cmd='advance'; ms=900} | Out-Null
  Ctl @{cmd='switch'; to='projects'} | Out-Null; Ctl @{cmd='advance'; ms=400} | Out-Null
  Check 'a first run has one empty project' ((Mod).projects -eq 1 -and (Mod).tasks.Count -eq 0)

  'tasks'
  ClickAt (Hit 'Add')
  Check 'the add button takes the keyboard' ((Ctl @{cmd='state'}).state.keyboard -eq $true -and $null -ne (Mod).editing)
  TypeLine 'Write API tests @fri'
  $t = (Mod).tasks
  Check 'the task is saved with its title (the date word is not in it)' ($t.Count -eq 1 -and $t[0].title -eq 'Write API tests') ($t | ConvertTo-Json -Compress)
  Check 'and the typed date became its due date' ($null -ne $t[0].due)
  Check 'it is on disk at once' ((Get-Content (Join-Path $data 'projects.json') -Raw) -match 'Write API tests')
  ClickAt (Hit 'Add'); Ctl @{cmd='key'; key='char'; ch='x'} | Out-Null; Ctl @{cmd='key'; key='escape'} | Out-Null
  Check 'Escape throws the typing away' ((Mod).tasks.Count -eq 1 -and $null -eq (Mod).editing)
  ClickAt (Hit 'Status(*')
  Check 'the circle moves it on' ((Mod).tasks[0].status -eq 'Doing')

  'files'
  Ctl @{cmd='drop'; paths=@((Join-Path $files 'spec.pdf'), (Join-Path $files 'repo'), (Join-Path $files 'missing.txt'))} | Out-Null; Ctl @{cmd='advance'; ms=300} | Out-Null
  Check 'dropped files and folders are attached (what is not there is skipped)' ((Mod).files.Count -eq 2 -and (Mod).view -eq 'Files') ((Mod) | ConvertTo-Json -Compress -Depth 4)
  ClickAt (Hit 'OpenFile(*')
  Check 'a click opens the file' ((@($script:log) -join '|') -match 'open_path:.*(spec\.pdf|repo)') ((@($script:log) -join '|'))
  Check 'the files are saved too' ((Get-Content (Join-Path $data 'projects.json') -Raw) -match 'spec.pdf')

  'projects'
  ClickAt (Hit 'NewProject')
  TypeLine 'Thesis'
  Check 'a new project is added and shown' ((Mod).project -eq 'Thesis' -and (Mod).projects -eq 2)
  ClickAt (Hit 'NextProject')
  Check 'the name switches projects' ((Mod).project -eq 'My project' -and (Mod).tasks.Count -eq 1)

  'looks'
  $dib = Ctl @{cmd='dibcheck'}
  Check 'the window shows exactly what was drawn' ($dib.differing -eq 0)
  Ctl @{cmd='dump'; path=(Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..\out')).Path 'projects-gate.png')} | Out-Null
  Ctl @{cmd='quit'} | Out-Null
} finally { Start-Sleep -Milliseconds 300; if (-not $p.HasExited) { $p.Kill() } }
"`n$script:pass passed, $script:fail failed"
if ($script:fail -gt 0) { exit 1 }
