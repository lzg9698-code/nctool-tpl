# Stop only this repository's NCtool UI listener before restarting the launcher.
param(
    [ValidateRange(1, 65535)]
    [int]$Port = 8788
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$repositoryPrefix = $repositoryRoot.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar

function Get-UiListeners {
    @(Get-NetTCPConnection -State Listen -ErrorAction Stop |
        Where-Object { $_.LocalPort -eq $Port } |
        Select-Object -ExpandProperty OwningProcess -Unique)
}

try {
    $listenerIds = @(Get-UiListeners)
    if ($listenerIds.Count -eq 0) {
        Write-Host "[nctool] Port $Port is available."
        exit 0
    }

    # Validate every owner before stopping any process. Never kill by image name.
    $owners = @($listenerIds | ForEach-Object {
        $ownerId = $_
        $owner = Get-CimInstance Win32_Process -Filter "ProcessId = $ownerId" -ErrorAction Stop
        if (-not $owner -or -not $owner.ExecutablePath) {
            throw "Cannot verify the owner of port $Port (PID $ownerId). No process was stopped."
        }
        $executable = [IO.Path]::GetFullPath($owner.ExecutablePath)
        if ($owner.Name -ine 'nctool.exe' -or
            -not $executable.StartsWith($repositoryPrefix, [StringComparison]::OrdinalIgnoreCase) -or
            $owner.CommandLine -notmatch '(?i)(?:^|\s)"?ui"?(?:\s|$)') {
            throw "Port $Port is occupied by another program (PID $ownerId). No process was stopped."
        }
        $owner
    })

    foreach ($owner in $owners) {
        $ownerId = $owner.ProcessId
        # Guard against PID reuse between inspecting and stopping the listener.
        $current = Get-CimInstance Win32_Process -Filter "ProcessId = $ownerId" -ErrorAction Stop
        if (-not $current) { continue }
        if ($current.CreationDate -ne $owner.CreationDate -or
            $current.ExecutablePath -ne $owner.ExecutablePath) {
            throw "The owner of port $Port changed. Restart aborted."
        }
        Write-Host "[nctool] Stopping old UI (PID $ownerId, port $Port)."
        # Include this server's external plugin children so no orphan remains.
        & taskkill.exe /PID $ownerId /T /F | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Could not stop UI process $ownerId." }
    }

    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        if (@(Get-UiListeners).Count -eq 0) {
            Write-Host "[nctool] Port $Port released. Starting the new UI."
            exit 0
        }
        Start-Sleep -Milliseconds 200
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "Port $Port was not released. Restart aborted."
} catch {
    Write-Host "[nctool] $($_.Exception.Message)" -ForegroundColor Red
    exit 1
}
