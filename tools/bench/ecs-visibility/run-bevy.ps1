param([Parameter(Mandatory)][string]$Binary,
      [Parameter(Mandatory)][string]$Delivery,
      [Parameter(Mandatory)][string]$Extra,
      [int]$Core = 16)
$ErrorActionPreference = 'Stop'
[Diagnostics.Process]::GetCurrentProcess().ProcessorAffinity = [intptr]([long]1 -shl $Core)
foreach ($round in 1..3) {
    & $Binary $Delivery $Extra 2400 bevy > "bevy-run-$round.txt"
    if ($LASTEXITCODE) {throw "Mask comparison failed in round $round"}
}
$rows = foreach ($round in 0..5) {
    foreach ($offset in 0..2) {
        $mode = @('cpu-direct','cpu-compact','cpu-bevy')[($round+$offset)%3]
        $argsLine = '"{0}" "{1}" 2400 {2}' -f $Delivery,$Extra,$mode
        $p = Start-Process $Binary -NoNewWindow -PassThru -ArgumentList $argsLine -RedirectStandardOutput "bevy-digests.txt"
        $p.WaitForExit()
        if ($p.ExitCode) {throw "CPU run failed for $mode"}
        [pscustomobject]@{round=$round;mode=$mode;cpu_ms=$p.TotalProcessorTime.TotalMilliseconds}
        $p.Dispose()
    }
}
$rows | Export-Csv -NoTypeInformation bevy-cpu.csv
