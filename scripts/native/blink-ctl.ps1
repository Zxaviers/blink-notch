# Helpers to drive a running blink.exe through its control pipe (no cursor, no keyboard).
#   . .\scripts\native\blink-ctl.ps1
#   $c = Connect-Blink 'mytest'; Send-Blink $c @{cmd='state'}

function Connect-Blink([string]$Name, [int]$TimeoutMs = 8000) {
  $client = New-Object System.IO.Pipes.NamedPipeClientStream('.', "blink-control-$Name", [System.IO.Pipes.PipeDirection]::InOut)
  $client.Connect($TimeoutMs)
  $utf8 = New-Object System.Text.UTF8Encoding($false)
  [pscustomobject]@{ Pipe = $client; Reader = (New-Object System.IO.StreamReader($client, $utf8)); Writer = (New-Object System.IO.StreamWriter($client, $utf8)) }
}

function Send-Blink($Conn, $Command) {
  $Conn.Writer.WriteLine(($Command | ConvertTo-Json -Compress -Depth 8))
  $Conn.Writer.Flush()
  $line = $Conn.Reader.ReadLine()
  if ($null -eq $line) { throw 'pipe closed' }
  $line | ConvertFrom-Json
}

function Start-Blink([string]$Name, [hashtable]$Env = @{}, [string[]]$Arguments = @()) {
  $exe = Join-Path $PSScriptRoot '..\..\target\debug\blink.exe'
  if ($env:BLINK_EXE) { $exe = $env:BLINK_EXE }
  $psi = New-Object System.Diagnostics.ProcessStartInfo($exe)
  $psi.UseShellExecute = $false
  $psi.EnvironmentVariables['BLINK_CONTROL'] = $Name
  foreach ($k in $Env.Keys) { $psi.EnvironmentVariables[$k] = [string]$Env[$k] }
  foreach ($a in $Arguments) { $psi.ArgumentList.Add($a) }
  [System.Diagnostics.Process]::Start($psi)
}
