# A pretend music player for checking the music page against the real Windows "now playing" list.
# It tells Windows a song is playing (title, artist, cover, position) and makes no sound at all
# (it plays nothing: only the information is shared), like Spotify would. It ends by itself.
#   powershell -STA -File scripts\native\fake-player.ps1 -Cover cover.png [-Title ..] [-Artist ..] [-Seconds 40]
param([string]$Cover, [string]$Title = 'Paper Sky', [string]$Artist = 'Lumen Coast', [string]$Album = 'Summer Flows', [int]$Seconds = 40, [int]$LengthSeconds = 216)
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$null = [Windows.Media.Playback.MediaPlayer, Windows.Media.Playback, ContentType = WindowsRuntime]
$null = [Windows.Media.MediaPlaybackType, Windows.Media, ContentType = WindowsRuntime]
$null = [Windows.Media.MediaPlaybackStatus, Windows.Media, ContentType = WindowsRuntime]
$null = [Windows.Media.SystemMediaTransportControlsTimelineProperties, Windows.Media, ContentType = WindowsRuntime]
$null = [Windows.Storage.StorageFile, Windows.Storage, ContentType = WindowsRuntime]
$null = [Windows.Storage.Streams.RandomAccessStreamReference, Windows.Storage.Streams, ContentType = WindowsRuntime]

function Await($op, [type]$result) {
  $m = [System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' } | Select-Object -First 1
  $task = $m.MakeGenericMethod($result).Invoke($null, @($op))
  $task.Wait()
  $task.Result
}

# A player object only so Windows has a media session to show; its own commands are switched off
# and the state below is set by hand.
$player = New-Object Windows.Media.Playback.MediaPlayer
$player.CommandManager.IsEnabled = $false
$smtc = $player.SystemMediaTransportControls
$smtc.IsEnabled = $true
$smtc.IsPlayEnabled = $true; $smtc.IsPauseEnabled = $true; $smtc.IsNextEnabled = $true; $smtc.IsPreviousEnabled = $true
$updater = $smtc.DisplayUpdater
$updater.Type = [Windows.Media.MediaPlaybackType]::Music
$updater.MusicProperties.Title = $Title
$updater.MusicProperties.Artist = $Artist
$updater.MusicProperties.AlbumTitle = $Album
if ($Cover) {
  $file = Await ([Windows.Storage.StorageFile]::GetFileFromPathAsync((Resolve-Path $Cover).Path)) ([Windows.Storage.StorageFile])
  $updater.Thumbnail = [Windows.Storage.Streams.RandomAccessStreamReference]::CreateFromFile($file)
}
$updater.Update()
$smtc.PlaybackStatus = [Windows.Media.MediaPlaybackStatus]::Playing
$timeline = New-Object Windows.Media.SystemMediaTransportControlsTimelineProperties
$timeline.StartTime = [TimeSpan]::Zero
$timeline.MinSeekTime = [TimeSpan]::Zero
$timeline.EndTime = [TimeSpan]::FromSeconds($LengthSeconds)
$timeline.MaxSeekTime = [TimeSpan]::FromSeconds($LengthSeconds)
$timeline.Position = [TimeSpan]::FromSeconds(60)
$smtc.UpdateTimelineProperties($timeline)
"showing '$Title' by $Artist for $Seconds s"
Start-Sleep -Seconds $Seconds
$smtc.PlaybackStatus = [Windows.Media.MediaPlaybackStatus]::Closed
$player.Dispose()
'done'
