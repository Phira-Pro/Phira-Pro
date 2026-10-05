function Clear-OldPackages {
    param([string]$DeliveryRoot, [string]$Version)
    $current = [regex]::Match($Version, '^(\d+\.\d+\.\d+)-pro\.(\d+)$')
    if (-not $current.Success) { return }
    $oldestKept = [int]$current.Groups[2].Value - 1
    $base = [regex]::Escape($current.Groups[1].Value)
    $pattern = "^PhiraPro-v$base-pro\.(\d+)-(?:android-arm64-v8a\.apk|win64\.zip)(?:\.sha256|\.idsig)?$"
    $rootPath = [System.IO.Path]::GetFullPath($DeliveryRoot).TrimEnd('\')
    foreach ($entry in Get-ChildItem -LiteralPath $rootPath -File) {
        $old = [regex]::Match($entry.Name, $pattern)
        if (-not $old.Success -or [int]$old.Groups[1].Value -ge $oldestKept) { continue }
        $entryPath = [System.IO.Path]::GetFullPath($entry.FullName)
        if ([System.IO.Path]::GetDirectoryName($entryPath) -ne $rootPath) { throw 'Package cleanup path escaped the delivery directory' }
        # Only known package files; never delete unpacked directories or data.
        Remove-Item -LiteralPath $entryPath -Force
        Write-Output "已清理旧包：$($entry.Name)"
    }
}
