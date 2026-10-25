# AI agents through the real programs: blinkctl.exe talks to the notch over its real pipe, and the notch shows it.
# No cursor, no keyboard. Starts its own notch (scripted names), so one in use on this PC is not touched.
#   powershell -File scripts\native\gate-agents.ps1 [-Exe path\to\blink.exe]    (blinkctl.exe is looked for next to it)
param([string]$Exe = '')
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\blink-ctl.ps1"
if ($Exe) { $env:BLINK_EXE = (Resolve-Path $Exe).Path }
$ctl = Join-Path (Split-Path $env:BLINK_EXE) 'blinkctl.exe'
$script:pass = 0; $script:fail = 0
function Check([string]$what, [bool]$ok, [string]$detail = '') { if ($ok) { $script:pass++; "  ok    $what" } else { $script:fail++; "  FAIL  $what  $detail" } }
$data = Join-Path $env:TEMP 'blink-agents-data'; if (Test-Path $data) { Remove-Item $data -Recurse -Force }; New-Item -ItemType Directory -Force $data | Out-Null
$env:BLINK_IPC = 'gate-agents'
$p = Start-Blink 'agents' @{ BLINK_MANUAL_CLOCK = '1'; BLINK_DATA_DIR = $data; BLINK_TZ = 'Asia/Jakarta'; BLINK_DEMO = '1'; BLINK_IPC = 'gate-agents' }
try {
  $c = Connect-Blink 'agents'
  function Ctl($o) { Send-Blink $c $o }
  function Agents() { (Ctl @{cmd='agent_list'}).agents }
  function Run($argList) { $ErrorActionPreference = 'Continue'; $out = & $ctl @argList 2>&1; [pscustomobject]@{ Code = $LASTEXITCODE; Text = (($out | ForEach-Object { "$_" }) -join "`n") } }

  $m = Ctl @{cmd='modules'}
  Check 'agents is one of the pages, after projects' (($m.modules | ForEach-Object { $_.id }) -join ',' -eq 'clock,deadlines,controls,music,projects,shelf,agents,system') (($m.modules | ForEach-Object { $_.id }) -join ',')
  # The 3 h 13 m demo deadline is not close once the pill's hours are 1.
  Ctl @{cmd='host'; config=@{modules=@(@{id='clock'; enabled=$true}, @{id='deadlines'; enabled=$true; options=@{urgentHours=1}}, @{id='agents'; enabled=$true})}} | Out-Null

  'the command reaches the notch'
  $r = Run @('ping'); Check 'ping answers with the version' ($r.Code -eq 0 -and $r.Text -match '^Blink ') $r.Text
  $r = Run @('agent', '--id', 'claude-1', '--name', 'Claude Code', '--state', 'running', '--title', 'Refactor the parser', '--progress', '0.4')
  Check 'an update is accepted' ($r.Code -eq 0) $r.Text
  $a = @(Agents)
  Check 'and listed with its state and title' ($a.Count -eq 1 -and $a[0].state -eq 'running' -and $a[0].title -eq 'Refactor the parser' -and [math]::Abs($a[0].progress - 0.4) -lt 0.01) ($a | ConvertTo-Json -Compress)
  Check 'listing from the command line shows it too' ((Run @('agent', '--list')).Text -match 'claude-1\s+running')

  'what it does about it'
  Ctl @{cmd='mouse'; kind='leave'} | Out-Null; Ctl @{cmd='advance'; ms=3000} | Out-Null
  Check 'a running agent leaves the clock on the pill' ((Ctl @{cmd='state'}).state.rest.label -eq '')
  $r = Run @('agent', '--id', 'claude-1', '--state', 'input', '--title', 'Run the tests?')
  Ctl @{cmd='advance'; ms=700} | Out-Null
  $st = (Ctl @{cmd='state'}).state
  Check 'one that needs you peeks' ($st.mode -eq 'peek' -and $st.peek.title -eq 'Run the tests?') "$($st.mode) $($st.peek | ConvertTo-Json -Compress)"
  Ctl @{cmd='advance'; ms=10000} | Out-Null
  $st = (Ctl @{cmd='state'}).state
  Check 'and takes the pill until it moves on' ($st.rest.label -eq 'Claude Code' -and $st.rest.count -eq 'need you') "$($st.rest | ConvertTo-Json -Compress)"
  $null = Run @('agent', '--id', 'claude-1', '--state', 'done', '--title', 'Finished')
  Ctl @{cmd='advance'; ms=300} | Out-Null
  Check 'when it is done the pill goes back to the clock' ((Ctl @{cmd='state'}).state.rest.label -eq '')

  'checks and exits'
  $r = Run @('agent', '--id', 'bad id', '--state', 'running'); Check 'a bad id is refused (exit 4)' ($r.Code -eq 4 -and $r.Text -match 'BAD_ID') "$($r.Code) $($r.Text)"
  $r = Run @('agent', '--id', 'a', '--url', 'http://insecure'); Check 'a link that is not https is refused' ($r.Code -eq 4 -and $r.Text -match 'BAD_URL')
  $r = Run @('agent', '--state', 'done'); Check 'a call without an id is a usage error (exit 2)' ($r.Code -eq 2)
  $r = Run @('agent', '--id', 'claude-1', '--remove'); Check 'removing works' ($r.Code -eq 0 -and @(Agents).Count -eq 0)
  $env:BLINK_IPC = 'gate-agents-nobody'
  $r = Run @('agent', '--id', 'a', '--state', 'running'); Check 'with no notch running: exit 3' ($r.Code -eq 3 -and $r.Text -match 'not running') "$($r.Code) $($r.Text)"
  $r = Run @('agent', '--id', 'a', '--state', 'running', '--quiet'); Check 'with --quiet it is a silent success' ($r.Code -eq 0 -and $r.Text -eq '')
  $env:BLINK_IPC = 'gate-agents'

  'hooks: one command for every event of an agent'
  function Hook($dialect, $json) { $ErrorActionPreference = 'Continue'; $out = $json | & $ctl hook $dialect 2>&1; [pscustomobject]@{ Code = $LASTEXITCODE; Text = (($out | ForEach-Object { "$_" }) -join "`n") } }
  $cwd = 'C:\Users\me\projects\parser'
  $r = Hook 'claude' (@{ hook_event_name = 'UserPromptSubmit'; session_id = 'sess1111aaaa'; cwd = $cwd } | ConvertTo-Json -Compress)
  $a = @(Agents)
  Check 'a Claude Code prompt shows a running agent named for the project' ($r.Code -eq 0 -and $a.Count -eq 1 -and $a[0].name -like 'Claude Code*parser' -and $a[0].state -eq 'running') ($a | ConvertTo-Json -Compress)
  $null = Hook 'claude' (@{ hook_event_name = 'UserPromptSubmit'; session_id = 'sess2222bbbb'; cwd = 'C:\other\site' } | ConvertTo-Json -Compress)
  Check 'a second session is a second agent' (@(Agents).Count -eq 2)
  $null = Hook 'claude' (@{ hook_event_name = 'Notification'; session_id = 'sess1111aaaa'; cwd = $cwd; message = 'Claude needs your permission to use Bash' } | ConvertTo-Json -Compress)
  $first = @(Agents) | Where-Object { $_.name -like '*parser' }
  Check 'a notification says it needs you, in its own words' ($first.state -eq 'input' -and $first.title -eq 'Claude needs your permission to use Bash') ($first | ConvertTo-Json -Compress)
  $null = Hook 'claude' (@{ hook_event_name = 'Stop'; session_id = 'sess1111aaaa'; cwd = $cwd } | ConvertTo-Json -Compress)
  Check 'Stop is finished' ((@(Agents) | Where-Object { $_.name -like '*parser' }).state -eq 'done')
  $null = Hook 'claude' (@{ hook_event_name = 'SessionEnd'; session_id = 'sess1111aaaa'; cwd = $cwd } | ConvertTo-Json -Compress)
  $null = Hook 'claude' (@{ hook_event_name = 'SessionEnd'; session_id = 'sess2222bbbb'; cwd = 'C:\other\site' } | ConvertTo-Json -Compress)
  Check 'the end of a session takes it off the list' (@(Agents).Count -eq 0)
  $g = Hook 'gemini' (@{ hook_event_name = 'BeforeAgent'; session_id = 'gem00000'; cwd = $cwd } | ConvertTo-Json -Compress)
  Check 'Gemini CLI has its own dialect and gets JSON back' ($g.Code -eq 0 -and $g.Text -eq '{}' -and (@(Agents) | Where-Object { $_.name -like 'Gemini CLI*' }).state -eq 'running') $g.Text
  $null = Hook 'gemini' (@{ hook_event_name = 'SessionEnd'; session_id = 'gem00000'; cwd = $cwd } | ConvertTo-Json -Compress)
  $r = Hook 'claude' 'not json at all'; Check 'a hook never fails its agent, whatever it is given' ($r.Code -eq 0 -and @(Agents).Count -eq 0)
  $r = Hook 'nope' '{}'; Check 'only an unknown dialect is an error' ($r.Code -eq 2)
  $env:BLINK_IPC = 'gate-agents-nobody'
  $r = Hook 'claude' (@{ hook_event_name = 'Stop' } | ConvertTo-Json -Compress); Check 'with no notch running a hook is a silent success' ($r.Code -eq 0 -and $r.Text -eq '')
  $env:BLINK_IPC = 'gate-agents'

  'run: any program, no setup'
  $r = Run @('run', '--id', 'job-ok', '--name', 'Build', '--', 'cmd', '/c', 'exit', '0')
  Check 'a program that succeeds ends as finished' ($r.Code -eq 0 -and (@(Agents) | Where-Object { $_.id -eq 'job-ok' }).state -eq 'done') ($r.Text)
  $r = Run @('run', '--id', 'job-bad', '--', 'cmd', '/c', 'exit', '3')
  Check 'one that fails ends as an error, with its own exit code' ($r.Code -eq 3 -and (@(Agents) | Where-Object { $_.id -eq 'job-bad' }).state -eq 'error' -and (@(Agents) | Where-Object { $_.id -eq 'job-bad' }).title -eq 'Exited with code 3')
  $r = Run @('run', '--id', 'job-none', '--', 'definitely-not-a-program-xyz'); Check 'a program that cannot start is said, and is an error on the notch' ($r.Code -eq 127 -and (@(Agents) | Where-Object { $_.id -eq 'job-none' }).state -eq 'error')
  foreach ($id in 'job-ok', 'job-bad', 'job-none') { $null = Run @('agent', '--id', $id, '--remove') }

  'mcp: for any agent that speaks it'
  $psi = New-Object System.Diagnostics.ProcessStartInfo; $psi.FileName = $ctl; $psi.Arguments = 'mcp'
  $psi.RedirectStandardInput = $true; $psi.RedirectStandardOutput = $true; $psi.UseShellExecute = $false; $psi.CreateNoWindow = $true
  $psi.EnvironmentVariables['BLINK_IPC'] = 'gate-agents'
  $mcp = [System.Diagnostics.Process]::Start($psi)
  function Rpc($o) { $mcp.StandardInput.WriteLine(($o | ConvertTo-Json -Compress -Depth 8)); $mcp.StandardInput.Flush() }
  function Reply() { $line = $mcp.StandardOutput.ReadLine(); if ($line) { $line | ConvertFrom-Json } }
  Rpc @{ jsonrpc = '2.0'; id = 1; method = 'initialize'; params = @{ protocolVersion = '2025-06-18'; clientInfo = @{ name = 'antigravity-client' }; capabilities = @{} } }
  $init = Reply
  Check 'the server introduces itself' ($init.result.serverInfo.name -eq 'blink')
  Rpc @{ jsonrpc = '2.0'; method = 'notifications/initialized' }
  Rpc @{ jsonrpc = '2.0'; id = 2; method = 'tools/list' }
  $list = Reply
  Check 'it offers the one tool' ($list.result.tools.Count -eq 1 -and $list.result.tools[0].name -eq 'blink_report_status')
  Rpc @{ jsonrpc = '2.0'; id = 3; method = 'tools/call'; params = @{ name = 'blink_report_status'; arguments = @{ state = 'input'; title = 'Approve the plan?' } } }
  $call = Reply
  $m = @(Agents) | Where-Object { $_.id -like 'mcp-antigravityclient-*' }
  Check 'a call shows up on the notch under the client''s name' ($call.result.isError -ne $true -and $m.name -eq 'Antigravity Client' -and $m.state -eq 'input' -and $m.title -eq 'Approve the plan?') ($m | ConvertTo-Json -Compress)
  $mcp.StandardInput.Close()
  $null = $mcp.WaitForExit(5000)
  Check 'when the client closes the connection the agent leaves the list' (@(Agents).Count -eq 0)

  'the page'
  Ctl @{cmd='agent_update'; id='a1'; name='Gemini CLI'; state='input'; title='Apply the migration?'} | Out-Null
  Ctl @{cmd='mouse'; kind='move'; x=320; y=10} | Out-Null; Ctl @{cmd='advance'; ms=900} | Out-Null
  Ctl @{cmd='switch'; to='agents'} | Out-Null; Ctl @{cmd='advance'; ms=400} | Out-Null
  $dib = Ctl @{cmd='dibcheck'}
  Check 'the window shows exactly what was drawn' ($dib.differing -eq 0)
  Ctl @{cmd='dump'; path=(Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..\out')).Path 'agents-gate.png')} | Out-Null
  Ctl @{cmd='quit'} | Out-Null
} finally { Remove-Item Env:BLINK_IPC -ErrorAction SilentlyContinue; Start-Sleep -Milliseconds 300; if (-not $p.HasExited) { $p.Kill() } }
"`n$script:pass passed, $script:fail failed"
if ($script:fail -gt 0) { exit 1 }
