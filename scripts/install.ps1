# Installs lazyp4 from a GitHub release. Needs no Rust.
#
#   irm https://raw.githubusercontent.com/linus-skold/lazyp4/main/scripts/install.ps1 | iex
#
# $env:LAZYP4_VERSION      a tag such as v0.2.0. Default: the latest release
# $env:LAZYP4_INSTALL_DIR  where the binary goes. Default: %LOCALAPPDATA%\Programs\lazyp4
#
# Works in Windows PowerShell 5.1 as well as PowerShell 7.

# A script block, so that `iex` leaves no variables in the caller's session,
# and `throw` rather than `exit`, which would close the caller's window.
& {
    $ErrorActionPreference = 'Stop'
    # Windows PowerShell 5.1 downloads many times slower with the progress bar.
    $ProgressPreference = 'SilentlyContinue'

    $repo = 'linus-skold/lazyp4'
    $asset = 'lazyp4-windows-x86_64.zip'
    $version = if ($env:LAZYP4_VERSION) { $env:LAZYP4_VERSION } else { 'latest' }
    $installDir = if ($env:LAZYP4_INSTALL_DIR) { $env:LAZYP4_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\lazyp4' }

    # ARM64 Windows runs the x86_64 binary under emulation.
    if (-not [Environment]::Is64BitOperatingSystem) {
        throw 'lazyp4: there is no release for 32-bit Windows.'
    }

    $base = if ($version -eq 'latest') {
        "https://github.com/$repo/releases/latest/download"
    } else {
        "https://github.com/$repo/releases/download/$version"
    }

    # Windows PowerShell 5.1 can default to a TLS version GitHub refuses.
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ('lazyp4-' + [Guid]::NewGuid())
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        Write-Host "lazyp4: downloading $asset ($version)"
        $zip = Join-Path $tmp $asset
        $sums = Join-Path $tmp 'SHA256SUMS'
        try {
            Invoke-WebRequest -UseBasicParsing -Uri "$base/$asset" -OutFile $zip
            Invoke-WebRequest -UseBasicParsing -Uri "$base/SHA256SUMS" -OutFile $sums
        } catch {
            throw "lazyp4: could not download from $base. Is there a published release? $_"
        }

        $line = Get-Content $sums | Where-Object { (($_ -split '\s+')[1] -replace '^\*', '') -eq $asset } | Select-Object -First 1
        if (-not $line) {
            throw "lazyp4: SHA256SUMS has no entry for $asset."
        }
        $expected = ($line -split '\s+')[0]
        $actual = (Get-FileHash -Algorithm SHA256 -Path $zip).Hash
        if ($actual -ne $expected) {
            throw "lazyp4: the checksum of $asset is wrong. Nothing was installed."
        }

        $extract = Join-Path $tmp 'extract'
        Expand-Archive -Path $zip -DestinationPath $extract
        New-Item -ItemType Directory -Force -Path $installDir | Out-Null

        # Windows locks a running executable against writes but not against a
        # rename, so move an existing one aside first.
        $exe = Join-Path $installDir 'lazyp4.exe'
        $old = "$exe.old"
        Remove-Item -Force $old -ErrorAction SilentlyContinue
        if (Test-Path $exe) {
            Move-Item -Force $exe $old
        }
        Copy-Item (Join-Path $extract 'lazyp4.exe') $exe
        Remove-Item -Force $old -ErrorAction SilentlyContinue
        Write-Host "lazyp4: installed $exe"

        # Read and write the registry value as it is stored. The Environment
        # class returns it with %VARIABLES% expanded and writes it back as a
        # plain string, which would freeze every entry that uses one.
        $added = $false
        $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
        try {
            $raw = [string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
            $entries = @($raw -split ';' | Where-Object { $_ })
            $present = $entries | Where-Object {
                [Environment]::ExpandEnvironmentVariables($_).TrimEnd('\') -eq $installDir.TrimEnd('\')
            }
            if (-not $present) {
                $key.SetValue('Path', (($entries + $installDir) -join ';'), [Microsoft.Win32.RegistryValueKind]::ExpandString)
                $added = $true
            }
        } finally {
            $key.Dispose()
        }

        if ($added) {
            # A write through the Environment class tells Explorer the
            # environment changed, so that new terminals see the new PATH.
            [Environment]::SetEnvironmentVariable('LAZYP4_INSTALL_NOTIFY', '1', 'User')
            [Environment]::SetEnvironmentVariable('LAZYP4_INSTALL_NOTIFY', $null, 'User')
            $env:Path = "$env:Path;$installDir"
            Write-Host "lazyp4: added $installDir to your user PATH. Open a new terminal to use it there."
        }
        Write-Host 'lazyp4: run it with: lazyp4'
    } finally {
        Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    }
}
