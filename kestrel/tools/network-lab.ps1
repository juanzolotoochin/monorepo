<#
.SYNOPSIS
Plays a Kestrel host and clients on this machine through a delaying UDP relay.

.DESCRIPTION
Starts a localhost relay (//kestrel/tools/network_lab:relay, a Rust program
built with Bazel), a host window and a client window:

    Client window <-> relay 127.0.0.1:<ProxyPort> <-> host 127.0.0.1:<HostPort>

The relay holds each datagram, in both directions, for
max(0, DelayMs + uniform(-JitterMs, +JitterMs)) ms. Datagrams may overtake one
another. Added round trip is about 2 x DelayMs; the game's input margin still
applies on top.

The client sizes its input margin from the round trip it measures. Pass
-InputMarginMs to pin it instead (the client gets --input-margin-ms; the host
is unaffected), for example 133 for the margin before it adapted.

-Clients starts that many client windows, up to 15. Each joins through the
relay with its own --profile, so the host sees separate players. A host
allows 8 players, so past 7 clients the rest are turned away as "full",
which checks the cap.

The launcher stays attached. Ctrl+C, or closing any game window, stops the
processes it started and nothing else. With -Rejoin, a client window that
closes is started again with the same name and profile, which checks leaving
and rejoining. Then only Ctrl+C or closing the host stops the run.
-RejoinAfterSeconds (default 1) sets how long it waits first, for example 30
to tell the effects of the leave and the rejoin apart. The relay forgets a client 3 s after it goes silent, so the rejoin's new
address gets a slot.

-ControlPortBase gives each window a game control port (JSON lines on
127.0.0.1): the host gets the base port and client n gets the base plus n.
A script can then ask any window for its state, for example its network
status and the other players it shows. Each run writes its parameters, process
IDs, relay counters and logs to network_lab/runs/<time>/.

It reuses the built game and relay. Pass -Build to build both first (once, no
tests), or -GamePath and -RelayPath for ones built elsewhere.

The relay polls during the last ~17 ms before each delivery, because Windows
timers wake in ~15.6 ms steps. That costs about one CPU core while packets are
in flight. -CoarseTimers saves the core but adds 0-15 ms to each datagram.

.EXAMPLE
.\kestrel\tools\network-lab.ps1 -DelayMs 75 -JitterMs 20 -Seed 42

.EXAMPLE
.\kestrel\tools\network-lab.ps1 -DelayMs 150 -JitterMs 20 -InputMarginMs 133

.EXAMPLE
.\kestrel\tools\network-lab.ps1 -Clients 7

.EXAMPLE
.\kestrel\tools\network-lab.ps1 -Clients 7 -Rejoin -ControlPortBase 7800
#>
[CmdletBinding()]
param(
    [string] $Scene = "laser-pit",
    [ValidateRange(0, 10000)] [int] $DelayMs = 75,
    [ValidateRange(0, 10000)] [int] $JitterMs = 20,
    [ValidateRange(0, 4294967295)] [long] $Seed = 42,
    [ValidateRange(1, 65535)] [int] $HostPort = 7777,
    [ValidateRange(1, 65535)] [int] $ProxyPort = 7778,
    # Unset: the client adapts its input margin to the network.
    [ValidateRange(0, 10000)] [Nullable[int]] $InputMarginMs,
    [ValidateRange(1, 15)] [int] $Clients = 1,
    # 0: no control ports.
    [ValidateRange(0, 65519)] [int] $ControlPortBase = 0,
    [switch] $Rejoin,
    [ValidateRange(1, 600)] [int] $RejoinAfterSeconds = 1,
    [switch] $Build,
    [switch] $CoarseTimers,
    [string] $GamePath,
    [string] $RelayPath
)

$ErrorActionPreference = "Stop"
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path

function Fail([string] $Message) {
    Write-Host "network-lab: $Message" -ForegroundColor Red
    exit 1
}

if ($HostPort -eq $ProxyPort) { Fail "-HostPort and -ProxyPort must differ." }
$ControlPorts = if ($ControlPortBase) { $ControlPortBase..($ControlPortBase + $Clients) } else { @() }
if ($ControlPorts -contains $HostPort -or $ControlPorts -contains $ProxyPort) {
    Fail "-ControlPortBase $ControlPortBase overlaps -HostPort or -ProxyPort."
}
if ($Scene -notmatch '^[a-z0-9-]+$') { Fail "-Scene must be a scene name such as laser-pit." }

foreach ($port in @($HostPort, $ProxyPort)) {
    $taken = Get-NetUDPEndpoint -LocalPort $port -ErrorAction SilentlyContinue
    if ($taken) {
        $owners = ($taken | ForEach-Object { $_.OwningProcess } | Sort-Object -Unique | ForEach-Object {
            $process = Get-Process -Id $_ -ErrorAction SilentlyContinue
            if ($process) { "$($process.ProcessName) ($_)" } else { "PID $_" }
        }) -join ", "
        Fail "UDP port $port is in use by $owners. Close it or pass -HostPort/-ProxyPort."
    }
}

$DefaultGame = Join-Path $RepoRoot "bazel-bin/kestrel/app/demo.exe"
$DefaultRelay = Join-Path $RepoRoot "bazel-bin/kestrel/tools/network_lab/relay.exe"
if ($Build) {
    if ($GamePath -or $RelayPath) { Fail "Pass -Build or -GamePath/-RelayPath, not both." }
    # `bazel run` builds for this machine (see .bazelrc); a plain remote
    # `bazel build` would produce a Linux binary. --script_path makes `run`
    # build and stop without starting the program. `run` takes one target,
    # so this runs twice; the second reuses the first's analysis.
    $script = Join-Path ([IO.Path]::GetTempPath()) "kestrel-network-lab-build.bat"
    foreach ($target in @("//kestrel/app:demo", "//kestrel/tools/network_lab:relay")) {
        Write-Host "Building $target (no tests)..."
        Push-Location $RepoRoot
        try {
            & bazel run --config=remote "--script_path=$script" $target
            $code = $LASTEXITCODE
        } finally {
            Pop-Location
            Remove-Item $script -ErrorAction SilentlyContinue
        }
        if ($code -ne 0) { Fail "Build of $target failed (bazel exited with $code)." }
    }
}

$Game = if ($GamePath) { $GamePath } else { $DefaultGame }
if (-not (Test-Path $Game -PathType Leaf)) {
    Fail "No game at $Game. Build it once with -Build, or pass -GamePath <path to demo.exe>."
}
$Game = (Resolve-Path $Game).Path
$GameBuilt = (Get-Item $Game).LastWriteTime
$Relay = if ($RelayPath) { $RelayPath } else { $DefaultRelay }
if (-not (Test-Path $Relay -PathType Leaf)) {
    Fail "No relay at $Relay. Build it once with -Build, or pass -RelayPath <path to relay.exe>."
}
$Relay = (Resolve-Path $Relay).Path
Write-Host "Game:  $Game"
Write-Host "Built: $($GameBuilt.ToString('yyyy-MM-dd HH:mm:ss'))"
Write-Host "Relay: $Relay"

$RunDir = Join-Path $PSScriptRoot ("network_lab/runs/" + (Get-Date -Format "yyyyMMdd-HHmmss"))
New-Item -ItemType Directory -Force $RunDir | Out-Null

$Started = [ordered]@{}
$Run = [ordered]@{
    started_at = (Get-Date).ToString("o")
    scene = $Scene
    delay_ms = $DelayMs
    jitter_ms = $JitterMs
    seed = $Seed
    # null: adaptive.
    input_margin_ms = $InputMarginMs
    clients = $Clients
    rejoin = [bool] $Rejoin
    rejoin_after_s = $RejoinAfterSeconds
    # 0: none. Otherwise the host's; client n has this plus n.
    control_port_base = $ControlPortBase
    # Client restarts with -Rejoin, by name.
    rejoins = [ordered]@{}
    coarse_timers = [bool] $CoarseTimers
    host_port = $HostPort
    proxy_port = $ProxyPort
    game = $Game
    game_built_at = $GameBuilt.ToString("o")
    relay = $Relay
    pids = $Started
}

function Save-Run {
    $Run | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $RunDir "run.json")
}

# Starts a process attached to this console (so no extra windows) with its
# output in the run directory, and remembers it for cleanup.
function Start-Part([string] $Name, [string] $Path, [string[]] $Arguments) {
    $quoted = $Arguments | ForEach-Object { if ($_ -match '[\s"]') { '"' + ($_ -replace '"', '\"') + '"' } else { $_ } }
    $process = Start-Process -FilePath $Path -ArgumentList $quoted -NoNewWindow -PassThru `
        -RedirectStandardOutput (Join-Path $RunDir "$Name.out.log") `
        -RedirectStandardError (Join-Path $RunDir "$Name.err.log")
    # Opening the handle now keeps the exit code readable after it exits.
    $null = $process.Handle
    $Started[$Name] = $process.Id
    Save-Run
    $process
}

function Show-Log([string] $Name) {
    foreach ($stream in @("err", "out")) {
        $log = Join-Path $RunDir "$Name.$stream.log"
        if ((Test-Path $log) -and (Get-Item $log).Length -gt 0) {
            Write-Host "--- $Name.$stream.log (last lines) ---"
            Get-Content $log -Tail 15 | ForEach-Object { Write-Host $_ }
        }
    }
}

function Read-Stats {
    $file = Join-Path $RunDir "stats.json"
    if (-not (Test-Path $file)) { return $null }
    try { Get-Content $file -Raw | ConvertFrom-Json } catch { $null }
}

function Format-Stats($stats) {
    if (-not $stats) { return "no relay stats yet" }
    $parts = foreach ($name in @("client_to_host", "host_to_client")) {
        $d = $stats.$name
        "{0}: {1} delivered, {2} queue-cap drops, {3} reordered, peak queue {4}, late p99 {5} ms" -f `
            $name, $d.delivered, $d.queue_cap_drops, $d.reordered, $d.peak_queued_packets, $d.lateness_ms.p99
    }
    ($parts + "stranger drops: $($stats.stranger_drops)") -join "`n  "
}

$processes = [ordered]@{}
$exitCode = 0
try {
    Save-Run
    $processes.relay = Start-Part "relay" $Relay @(
        "--listen", $ProxyPort, "--host", $HostPort,
        "--delay-ms", $DelayMs, "--jitter-ms", $JitterMs, "--seed", $Seed,
        "--spin-ms", $(if ($CoarseTimers) { 0 } else { 17 }), "--max-clients", $Clients,
        "--run-dir", $RunDir, "--parent-pid", $PID)
    $deadline = (Get-Date).AddSeconds(10)
    while (-not (Test-Path (Join-Path $RunDir "ready.json"))) {
        if ($processes.relay.HasExited) { Show-Log "relay"; throw "The relay exited during startup." }
        if ((Get-Date) -gt $deadline) { Show-Log "relay"; throw "The relay did not report ready within 10 s." }
        Start-Sleep -Milliseconds 100
    }

    $hostArgs = @("--host", $HostPort, "--scene", $Scene, "--name", "Host")
    if ($ControlPortBase) { $hostArgs += @("--control-port", $ControlPortBase) }
    $processes.host = Start-Part "host" $Game $hostArgs
    $deadline = (Get-Date).AddSeconds(60)
    while (-not (Get-NetUDPEndpoint -LocalPort $HostPort -OwningProcess $processes.host.Id -ErrorAction SilentlyContinue)) {
        if ($processes.host.HasExited) { Show-Log "host"; throw "The host exited during startup." }
        if ((Get-Date) -gt $deadline) { Show-Log "host"; throw "The host did not open UDP port $HostPort within 60 s." }
        Start-Sleep -Milliseconds 200
    }

    $clientArgs = [ordered]@{}
    # When each client was last started, as seen here.
    $startedAt = @{}
    foreach ($n in 1..$Clients) {
        # One client keeps the old names; several are numbered.
        $name = if ($Clients -eq 1) { "client" } else { "client$n" }
        $label = if ($Clients -eq 1) { "Client" } else { "Client $n" }
        $arguments = @("--join", "127.0.0.1:$ProxyPort", "--name", $label, "--profile", "networklab$n")
        if ($null -ne $InputMarginMs) { $arguments += @("--input-margin-ms", $InputMarginMs) }
        if ($ControlPortBase) { $arguments += @("--control-port", ($ControlPortBase + $n)) }
        $clientArgs[$name] = $arguments
        $processes[$name] = Start-Part $name $Game $arguments
        $startedAt[$name] = Get-Date
    }

    $margin = if ($null -ne $InputMarginMs) { "pinned at $InputMarginMs ms" } else { "adaptive" }
    Write-Host ""
    Write-Host "Running: $DelayMs ms +/- $JitterMs ms each way (seed $Seed); added RTT ~$(2 * $DelayMs) ms before jitter and polling."
    Write-Host "Clients: $Clients. Client input margin: $margin."
    if ($ControlPortBase) { Write-Host "Control ports: host $ControlPortBase, client n $ControlPortBase + n." }
    if ($Rejoin) { Write-Host "Rejoin: a client window that closes starts again. Ctrl+C or close the host to stop." }
    Write-Host "In any window press / then Diagnostics -> Network."
    Write-Host "All windows share this machine's CPU/GPU: read frame time alongside the network graphs."
    Write-Host "Run directory: $RunDir"
    if (-not $Rejoin) { Write-Host "Ctrl+C or close a game window to stop." }

    $nextReport = (Get-Date).AddSeconds(15)
    while ($true) {
        $ended = @($processes.Keys | Where-Object { $processes[$_].HasExited })
        if ($Rejoin) {
            foreach ($name in @($ended | Where-Object { $clientArgs.Contains($_) })) {
                $process = $processes[$name]
                $code = $process.ExitCode
                # One that closes soon after starting (it failed, or the game
                # was full) would only do it again: leave it closed.
                if (((Get-Date) - $startedAt[$name]).TotalSeconds -lt 15) {
                    Write-Host "$name exited (code $code) within 15 s of starting; not starting it again."
                    Show-Log $name
                    $processes.Remove($name)
                    continue
                }
                Write-Host "$name exited (code $code); starting it again in $RejoinAfterSeconds s."
                if ($code -ne 0) { Show-Log $name }
                # Keep each start's logs: the restart writes to "<name>-<k>".
                $k = 1 + $(if ($Run.rejoins.Contains($name)) { $Run.rejoins[$name] } else { 0 })
                $Run.rejoins[$name] = $k
                # The launcher only waits here; the windows keep running.
                Start-Sleep -Seconds $RejoinAfterSeconds
                $processes[$name] = Start-Part "$name-$k" $Game $clientArgs[$name]
                $startedAt[$name] = Get-Date
            }
            $ended = @($ended | Where-Object { -not $clientArgs.Contains($_) })
        }
        if ($ended) {
            foreach ($name in $ended) {
                Write-Host "$name exited (code $($processes[$name].ExitCode))."
                if ($processes[$name].ExitCode -ne 0) { Show-Log $name; $exitCode = 1 }
            }
            break
        }
        if ((Get-Date) -gt $nextReport) {
            Write-Host ("relay  " + (Format-Stats (Read-Stats)))
            $nextReport = (Get-Date).AddSeconds(15)
        }
        Start-Sleep -Milliseconds 500
    }
} catch {
    Write-Host "network-lab: $($_.Exception.Message)" -ForegroundColor Red
    $exitCode = 1
} finally {
    # Ask the relay to write its final stats, then stop whatever is left of
    # what this launcher started.
    New-Item -ItemType File -Force (Join-Path $RunDir "stop") | Out-Null
    if ($processes.relay) { $processes.relay.WaitForExit(3000) | Out-Null }
    foreach ($process in $processes.Values) {
        if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue }
    }
    $Run.ended_at = (Get-Date).ToString("o")
    Save-Run
    Write-Host "Stopped. Relay totals:"
    Write-Host ("  " + (Format-Stats (Read-Stats)))
    Write-Host "Logs and counters: $RunDir"
}
exit $exitCode
