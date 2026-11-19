# Install Arugula on Windows: download a release, check it, and run
# `arugulad install`, which puts arugulad and arugula in
# %LOCALAPPDATA%\Programs\arugula (on your PATH) and starts the daemon at
# logon as a scheduled task. Run it again to upgrade; the daemon's flags and
# your panes are kept.
#
#   irm https://arugula.io/install.ps1 | iex
#
# arugula.io serves this file as the newest daemon release's commit has it
# (arugula-salad/site's Worker, cached 5 minutes): a change here ships with
# the next release, not when it merges.
#
# $env:ARUGULA_VERSION = 'vX.Y.Z'   a release tag (default: the latest)
# $env:ARUGULA_NO_START = '1'        register the task without starting it
# $env:ARUGULA_DOWNLOAD_URL = '...'  where the release's files are, instead
#                                      of GitHub (a mirror, or a local folder)
# $env:ARUGULA_ALLOW_UNSIGNED = '1'  install a release whose SHA256SUMS's
#                                      signature can't be checked: one from
#                                      before releases were signed, a local
#                                      build, or a machine without ssh-keygen
#                                      from OpenSSH 8.1 or later

# A block of its own: `iex` runs this in your shell, which an `exit` would end.
& {
  $ErrorActionPreference = 'Stop'
  $ProgressPreference = 'SilentlyContinue'
  $repo = 'https://github.com/arugula-salad/arugula'
  # ARUGULA_X. Read, not set: this runs in your shell.
  $setting = {
    param($n)
    [Environment]::GetEnvironmentVariable("ARUGULA_$n")
  }

  # The SSH key that signs a release's SHA256SUMS (SHA256SUMS.sshsig,
  # `ssh-keygen -Y sign`, namespace arugula-release; install.sh and
  # scripts/release have the same key), as an allowed-signers line, and its
  # signature of 'arugula release key', to tell whether this machine's
  # ssh-keygen can check signatures at all (-Y verify is OpenSSH 8.1 or
  # later). Windows 10 and later have OpenSSH's ssh-keygen.
  $sumsKey = 'arugula-release ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIIQvxZLSOJnbNQGrK2Q72gdNqHXBmDtixUemY9RCpsKs'
  $sumsProbe = @'
-----BEGIN SSH SIGNATURE-----
U1NIU0lHAAAAAQAAADMAAAALc3NoLWVkMjU1MTkAAAAghC/FktI4mds1AasrZDvaB02odc
GYO2LFR6Zj1EKmwqwAAAAPYXJ1Z3VsYS1yZWxlYXNlAAAAAAAAAAZzaGE1MTIAAABTAAAA
C3NzaC1lZDI1NTE5AAAAQJdemiJv1aEH10IKH/NSs8FbDIgzFvtIbwVy9817CSrDRCvl3I
IXO9z3CPM1HBnm8CgJnkc8eFVrPQaVOEMQEgA=
-----END SSH SIGNATURE-----
'@
  # A test's own key: KEY.ssh (an allowed-signers line) and KEY.ssh.probe,
  # as scripts/tests/install.sh gives install.sh.
  $testKey = & $setting 'TEST_RELEASE_KEY'
  if ($testKey) {
    $sumsKey = (Get-Content -Raw "$testKey.ssh").Trim()
    $sumsProbe = Get-Content -Raw "$testKey.ssh.probe"
  }

  # x64 builds; Windows on Arm runs them too.
  if (-not [Environment]::Is64BitOperatingSystem) { throw 'arugula: there is no 32-bit Windows release' }
  $target = 'x86_64-pc-windows-msvc'

  $version = & $setting 'VERSION'
  if (-not $version) {
    # releases/latest redirects to releases/tag/<tag>. Not GitHub's API: its
    # unauthenticated limit is shared by everyone behind an IP.
    $req = [System.Net.HttpWebRequest]::Create("$repo/releases/latest")
    $req.AllowAutoRedirect = $false
    $res = $req.GetResponse()
    $version = ($res.Headers['Location'] -split '/releases/tag/')[-1]
    $res.Close()
    if ($version -notmatch '^v[0-9]') { throw "arugula: couldn't find the latest release at $repo/releases" }
  }

  $name = "arugula-$($version.TrimStart('v'))-$target"
  $base = if (& $setting 'DOWNLOAD_URL') { & $setting 'DOWNLOAD_URL' } else { "$repo/releases/download/$version" }
  $tmp = Join-Path ([IO.Path]::GetTempPath()) "arugula-install-$PID"
  New-Item -ItemType Directory -Force $tmp | Out-Null
  try {
    "downloading $name"
    $get = { param($file) if (Test-Path $base) { Copy-Item (Join-Path $base $file) $tmp } else { Invoke-WebRequest "$base/$file" -OutFile (Join-Path $tmp $file) -UseBasicParsing } }
    & $get "$name.zip"
    & $get 'SHA256SUMS'

    # SHA256SUMS must be the release key's: whoever could swap the zip could
    # swap SHA256SUMS too, but not sign it. ssh-keygen checks it, reading
    # the signed file on its input, byte for byte.
    $sshVerify = {
      param($keygen, $data, $sig)
      $psi = New-Object System.Diagnostics.ProcessStartInfo
      $psi.FileName = $keygen
      $psi.Arguments = "-Y verify -f `"$(Join-Path $tmp 'allowed_signers')`" -I arugula-release -n arugula-release -s `"$sig`""
      $psi.UseShellExecute = $false
      $psi.RedirectStandardInput = $true
      $psi.RedirectStandardOutput = $true
      $psi.RedirectStandardError = $true
      $p = [System.Diagnostics.Process]::Start($psi)
      $bytes = [IO.File]::ReadAllBytes($data)
      # One that stops before reading it (no -Y) closes the pipe: a failure,
      # as its exit code says.
      try {
        $p.StandardInput.BaseStream.Write($bytes, 0, $bytes.Length)
        $p.StandardInput.Close()
      } catch { }
      $null = $p.StandardOutput.ReadToEnd()
      $null = $p.StandardError.ReadToEnd()
      $p.WaitForExit()
      $p.ExitCode -eq 0
    }
    # 0: signed by the release key; 1: not; 2: no ssh-keygen here that can
    # tell; 3: the release has no SHA256SUMS.sshsig.
    $rc = 3
    $sums = Join-Path $tmp 'SHA256SUMS'
    $sig = Join-Path $tmp 'SHA256SUMS.sshsig'
    $haveSig = $true
    try { & $get 'SHA256SUMS.sshsig' } catch { $haveSig = $false }
    if ($haveSig) {
      $rc = 2
      $keygen = Get-Command ssh-keygen -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
      if ($keygen) {
        [IO.File]::WriteAllText((Join-Path $tmp 'allowed_signers'), "$sumsKey`n")
        [IO.File]::WriteAllText((Join-Path $tmp 'probe'), 'arugula release key')
        [IO.File]::WriteAllText((Join-Path $tmp 'probe.sshsig'), (($sumsProbe -replace "`r", '').Trim() + "`n"))
        if (& $sshVerify $keygen.Source (Join-Path $tmp 'probe') (Join-Path $tmp 'probe.sshsig')) {
          $rc = if (& $sshVerify $keygen.Source $sums $sig) { 0 } else { 1 }
        }
      }
    }
    if ($rc -eq 1) { throw "arugula: release $version's SHA256SUMS isn't signed by the release key: not installing it" }
    if ($rc -ne 0) {
      $why = if ($rc -eq 2) {
        "can't check the signature of release $version's SHA256SUMS here: that takes ssh-keygen from OpenSSH 8.1 or later (Settings > System > Optional features > OpenSSH Client)"
      } else {
        "release $version's SHA256SUMS isn't signed (no SHA256SUMS.sshsig)"
      }
      if ((& $setting 'ALLOW_UNSIGNED') -eq '1') {
        "${why}: installing it anyway (ARUGULA_ALLOW_UNSIGNED=1)"
      } else {
        throw "arugula: $why. `$env:ARUGULA_ALLOW_UNSIGNED = '1' installs it anyway"
      }
    }

    $line = Get-Content (Join-Path $tmp 'SHA256SUMS') | Where-Object { $_ -match " $([regex]::Escape("$name.zip"))$" } | Select-Object -First 1
    if (-not $line) { throw "arugula: $name.zip isn't in SHA256SUMS" }
    $want = ($line -split '\s+')[0].ToLower()
    $got = (Get-FileHash (Join-Path $tmp "$name.zip") -Algorithm SHA256).Hash.ToLower()
    if ($want -ne $got) { throw "arugula: checksum mismatch for $name.zip" }

    Expand-Archive (Join-Path $tmp "$name.zip") $tmp -Force
    $daemon = Join-Path $tmp "$name\arugulad.exe"
    if (& $setting 'NO_START') { & $daemon install --no-start } else { & $daemon install }
    if ($LASTEXITCODE -ne 0) { throw "arugula: arugulad install failed ($LASTEXITCODE)" }

    $bin = Join-Path $env:LOCALAPPDATA 'Programs\arugula'
    ''
    'Arugula is installed. In a new terminal:'
    '  arugula web             open it in your browser, signed in'
    '  arugula run powershell  a pane, from the command line'
    "(Or now: & '$bin\arugula.exe' web)"
    # Telemetry is opt-in (docs/telemetry.md): say the first command asks,
    # unless the environment already keeps it off.
    if (-not $env:ARUGULA_TELEMETRY -and -not $env:DO_NOT_TRACK) {
      ''
      "Usage reports are off. The first ``arugula`` command asks if you'd like to send them (set ARUGULA_TELEMETRY=off to never be asked)."
    }
  } finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
  }
}
