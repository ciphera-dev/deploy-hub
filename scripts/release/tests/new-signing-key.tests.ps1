#Requires -Version 7.0
# Tests de scripts/release/new-signing-key.ps1 sans vraie cle ni reseau :
# minisign, gh et Read-Host sont remplaces par des fonctions (prioritaires
# sur les commandes externes), le depot par un faux dossier.
#
#   pwsh -NoProfile -File scripts/release/tests/new-signing-key.tests.ps1
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$script = Join-Path $PSScriptRoot '..' 'new-signing-key.ps1'
$testPubSource = Join-Path $PSScriptRoot '..' '..' '..' 'install' 'tests' 'ciphera-test.pub'
$failures = 0

function Assert([string]$Label, [scriptblock]$Condition) {
    if (& $Condition) { Write-Host "ok   $Label" }
    else { Write-Host "FAIL $Label"; $script:failures++ }
}

function Assert-Throws([string]$Label, [string]$Pattern, [scriptblock]$Action) {
    try { & $Action *> $null; Write-Host "FAIL $Label (aucune erreur)"; $script:failures++ }
    catch {
        if ($_.Exception.Message -match $Pattern) { Write-Host "ok   $Label" }
        else { Write-Host "FAIL $Label : $($_.Exception.Message)"; $script:failures++ }
    }
}

$work = Join-Path ([IO.Path]::GetTempPath()) ('ciphera-key-test-' + [Guid]::NewGuid().ToString('N'))
$fakeRepo = Join-Path $work 'repo'
$pubTarget = Join-Path $fakeRepo 'install' 'ciphera.pub'
New-Item -ItemType Directory -Path (Join-Path $fakeRepo 'install' 'tests') -Force | Out-Null
Copy-Item $testPubSource $pubTarget
Copy-Item $testPubSource (Join-Path $fakeRepo 'install' 'tests' 'ciphera-test.pub')
$password = 'un mot de passe tres long'
$global:expectedPassword = $password

# --- bouchons -----------------------------------------------------------------
# minisign : -G cree une cle unique (RW + 54 caracteres) et une cle secrete
# « SECRET:<cle> » ; -S exige le bon mot de passe et signe avec la cle de la
# cle secrete ; -V -P <cle> verifie que la signature est de <cle>.
$global:calls = [Collections.Generic.List[object]]::new()
$global:secretExists = $false
$global:failSecret = ''
$global:keyCounter = 0
$global:typed = @()
function global:Read-Host {
    param([switch]$AsSecureString, [Parameter(Position = 0)]$Prompt)
    $v = $global:typed[0]
    $global:typed = @($global:typed | Select-Object -Skip 1)
    ConvertTo-SecureString $v -AsPlainText -Force
}
function global:minisign {
    $stdin = @($input) -join "`n"
    $global:calls.Add([pscustomobject]@{ Tool = 'minisign'; Args = @($args); Stdin = $stdin })
    $global:LASTEXITCODE = 0
    switch ($args[0]) {
        '-G' {
            $global:keyCounter++
            $key = 'RW' + ('A' * 50) + ('{0:D4}' -f $global:keyCounter)
            Set-Content -LiteralPath $args[2] -Value "untrusted comment: minisign public key $($global:keyCounter)`n$key" -NoNewline
            Set-Content -LiteralPath $args[4] -Value "untrusted comment: minisign encrypted secret key`r`nSECRET:$key" -NoNewline
        }
        '-S' {
            if ($stdin -cne $global:expectedPassword) { $global:LASTEXITCODE = 1; return "Wrong password" }
            $key = ((Get-Content -LiteralPath $args[2] -Raw) -split 'SECRET:')[1].Trim()
            Set-Content -LiteralPath ($args[4] + '.minisig') -Value $key -NoNewline
        }
        '-V' {
            $sig = Get-Content -LiteralPath ($args[5] + '.minisig') -Raw
            if ($sig -cne $args[3]) { $global:LASTEXITCODE = 1 }
        }
    }
}
function global:gh {
    $stdin = @($input) -join "`n"
    $global:calls.Add([pscustomobject]@{ Tool = 'gh'; Args = @($args); Stdin = $stdin })
    $global:LASTEXITCODE = 0
    if ($args[0] -eq 'secret' -and $args[1] -eq 'list' -and $global:secretExists) { return 'CIPHERA_MINISIGN_SECRET_KEY' }
    if ($args[0] -eq 'secret' -and $args[1] -eq 'set' -and $args[2] -eq $global:failSecret) { $global:LASTEXITCODE = 1; return 'HTTP 500' }
    if ($args[0] -eq 'api' -and $args[1] -like '*/deployment-branch-policies') { return $global:policiesJson }
    if ($args[0] -eq 'api' -and $args[1] -like '*/environments/release') { return $global:envJson }
    if ($args[0] -eq 'release' -and $args[1] -eq 'view') {
        if (-not $global:latestTag) { $global:LASTEXITCODE = 1; return 'release not found' }
        return $global:latestTag
    }
    if ($args[0] -eq 'release' -and $args[1] -eq 'download') { return @('#!/bin/sh', "CIPHERA_PUBKEYS='$global:latestKeys'", 'main') }
}
# Environnement `release` conforme (relecteur, tag v* seulement), puis
# variantes non conformes.
$global:goodEnv = '{"protection_rules":[{"type":"required_reviewers","reviewers":[{"type":"User","reviewer":{"login":"duckysface"}}]},{"type":"branch_policy"}],"deployment_branch_policy":{"protected_branches":false,"custom_branch_policies":true}}'
$global:goodPolicies = '{"branch_policies":[{"name":"v*","type":"tag"}]}'
$global:envJson = $global:goodEnv
$global:policiesJson = $global:goodPolicies
$global:latestTag = ''
$global:latestKeys = ''

. $script -Repo 'ciphera-dev/deploy-hub'
function Get-RepoRoot { $fakeRepo }
function Use-Mode([string]$Mode, [string]$Out = '', [string]$Key = '') {
    $script:Rotate = [switch]($Mode -eq 'Rotate')
    $script:Activate = [switch]($Mode -eq 'Activate')
    $script:Verify = [switch]($Mode -eq 'Verify')
    $script:OutDir = $Out
    $script:SecretKeyFile = $Key
}
function Get-Sets { @($global:calls | Where-Object { $_.Tool -eq 'gh' -and $_.Args[1] -eq 'set' }) }

try {
    # --- fonctions ----------------------------------------------------------------
    Assert 'chemin dans le depot detecte' { Test-PathInside (Join-Path $fakeRepo 'x') $fakeRepo }
    Assert 'chemin voisin non confondu' { -not (Test-PathInside ($fakeRepo + '-bis') $fakeRepo) }
    Push-Location $work
    try { Assert 'chemin relatif resolu depuis $PWD' { (Resolve-UserPath 'rel') -eq (Join-Path $work 'rel') } }
    finally { Pop-Location }
    Assert 'cle de test lue' { (Get-PublicKeyLine $testPubSource) -like 'RW*' }
    $f = Join-Path $work 'wipe.bin'
    Set-Content -LiteralPath $f -Value 'abc'
    Remove-FileSecurely $f
    Assert 'fichier efface' { -not (Test-Path -LiteralPath $f) }

    # --- premiere cle : refus -------------------------------------------------------
    Use-Mode Init (Join-Path $fakeRepo 'cle')
    Assert-Throws 'OutDir dans le depot refuse' 'hors du depot' { Invoke-Main }
    $global:typed = @($password, 'autre mot de passe long')
    Use-Mode Init (Join-Path $work 'out-mismatch')
    Assert-Throws 'mots de passe differents refuses' 'different' { Invoke-Main }
    $global:typed = @('court', 'court')
    Use-Mode Init (Join-Path $work 'out-short')
    Assert-Throws 'mot de passe court refuse' 'trop court' { Invoke-Main }
    $global:secretExists = $true
    Use-Mode Init (Join-Path $work 'out-exists')
    Assert-Throws 'secret existant : -Activate indique' 'Activate' { Invoke-Main }
    Assert-Throws 'secret existant : suppression indiquee si cle abandonnee' 'gh secret delete CIPHERA_MINISIGN_SECRET_KEY --env release' { Invoke-Main }
    $global:secretExists = $false

    # --- environnement release non protege -------------------------------------------
    $global:envJson = '{"protection_rules":[],"deployment_branch_policy":{"protected_branches":false,"custom_branch_policies":true}}'
    Use-Mode Init (Join-Path $work 'out-env1')
    Assert-Throws 'environnement sans relecteur refuse' 'aucun relecteur' { Invoke-Main }
    $global:envJson = $global:goodEnv -replace '"deployment_branch_policy":\{[^}]*\}', '"deployment_branch_policy":null'
    Use-Mode Init (Join-Path $work 'out-env2')
    Assert-Throws 'environnement sans limite de deploiement refuse' 'non limites' { Invoke-Main }
    $global:envJson = $global:goodEnv
    $global:policiesJson = '{"branch_policies":[{"name":"v*","type":"tag"},{"name":"main","type":"branch"}]}'
    Use-Mode Init (Join-Path $work 'out-env3')
    Assert-Throws 'environnement ouvert a une branche refuse' 'exactement : tag v\*' { Invoke-Main }
    $global:policiesJson = $global:goodPolicies
    Assert 'environnement refuse : aucun secret envoye' { @(Get-Sets).Count -eq 0 }

    # --- premiere cle : envoi interrompu puis reprise ------------------------------
    $global:calls.Clear()
    $global:typed = @($password, $password)
    $global:failSecret = 'CIPHERA_MINISIGN_PASSWORD'
    $out1 = Join-Path $work 'out1'
    Use-Mode Init $out1
    $err = ''
    try { Invoke-Main *> $null } catch { $err = $_.Exception.Message }
    $key1 = Get-PublicKeyLine (Join-Path $out1 'ciphera-release.pub')
    Assert 'envoi interrompu : secrets deja envoyes indiques' { $err -match 'deja envoyes : CIPHERA_MINISIGN_SECRET_KEY' }
    Assert 'envoi interrompu : reprise -Activate indiquee' { $err -match '-Activate -SecretKeyFile' }
    Assert 'envoi interrompu : cle publique deja ecrite' { (Get-PublicKeyLines $pubTarget) -ceq $key1 }
    $global:failSecret = ''

    $global:calls.Clear()
    $global:typed = @($password)
    Use-Mode Activate '' (Join-Path $out1 'ciphera-release.key')
    Invoke-Main *> $null
    $sets = Get-Sets
    Assert 'reprise : deux secrets envoyes dans l''environnement release' {
        $sets.Count -eq 2 -and @($sets | Where-Object { ($_.Args -join ' ') -notmatch '--env release --repo ciphera-dev/deploy-hub' }).Count -eq 0
    }
    Assert 'cle secrete envoyee par l''entree standard (LF)' {
        ($sets | Where-Object { $_.Args[2] -eq 'CIPHERA_MINISIGN_SECRET_KEY' }).Stdin -ceq "untrusted comment: minisign encrypted secret key`nSECRET:$key1"
    }
    Assert 'mot de passe envoye par l''entree standard' {
        ($sets | Where-Object { $_.Args[2] -eq 'CIPHERA_MINISIGN_PASSWORD' }).Stdin -ceq $password
    }

    # --- premiere cle : chemin nominal ---------------------------------------------
    Copy-Item $testPubSource $pubTarget -Force
    $global:calls.Clear()
    $global:typed = @($password, $password)
    $out2 = Join-Path $work 'out2'
    Use-Mode Init $out2
    Invoke-Main *> $null
    $key2 = Get-PublicKeyLine (Join-Path $out2 'ciphera-release.pub')
    Assert 'install/ciphera.pub = seule la cle generee' {
        (Get-Content -LiteralPath $pubTarget -Raw) -ceq "untrusted comment: minisign public key $($global:keyCounter)`n$key2`n"
    }
    Assert 'aucune cle secrete dans le depot' { -not (Get-ChildItem -LiteralPath $fakeRepo -Recurse -File | Select-String -Pattern 'SECRET') }
    Assert 'deux secrets envoyes' { @(Get-Sets).Count -eq 2 }
    Assert 'mot de passe jamais en argument' {
        -not ($global:calls | Where-Object { ($_.Args -join ' ') -match [regex]::Escape($password) })
    }
    Assert 'generation : mot de passe deux fois sur l''entree standard' {
        ($global:calls | Where-Object { $_.Tool -eq 'minisign' -and $_.Args[0] -eq '-G' }).Stdin -ceq "$password`n$password"
    }
    Assert 'cle secrete gardee dans OutDir pour la sauvegarde' { Test-Path -LiteralPath (Join-Path $out2 'ciphera-release.key') }
    Use-Mode Init (Join-Path $work 'out-again')
    Assert-Throws 'cle de production en place : -Rotate indique' 'Rotate' { Invoke-Main }

    # --- rotation : nouvelle cle ajoutee, rien n'est envoye -------------------------
    $global:calls.Clear()
    $global:typed = @($password, $password)
    $out3 = Join-Path $work 'out3'
    Use-Mode Rotate $out3
    Invoke-Main *> $null
    $key3 = Get-PublicKeyLine (Join-Path $out3 'ciphera-release.pub')
    $keys = Get-PublicKeyLines $pubTarget
    Assert 'rotation : ancienne et nouvelle cles de confiance' { $keys.Count -eq 2 -and $keys[0] -ceq $key2 -and $keys[1] -ceq $key3 }
    Assert 'rotation : aucun secret envoye' { @(Get-Sets).Count -eq 0 }

    # -Verify d'une copie : aucun appel a gh, rien n'est envoye.
    $global:calls.Clear()
    $global:typed = @($password)
    Use-Mode Verify '' (Join-Path $out3 'ciphera-release.key')
    Invoke-Main *> $null
    Assert 'verification seule : aucun appel a gh' { @($global:calls | Where-Object { $_.Tool -eq 'gh' }).Count -eq 0 }
    $global:typed = @('mauvais mot de passe long')
    Use-Mode Verify '' (Join-Path $out3 'ciphera-release.key')
    Assert-Throws 'verification seule : mauvais mot de passe refuse' 'minisign a echoue' { Invoke-Main }

    # -Activate avant la release N (qui distribue la nouvelle cle) : refuse.
    $global:calls.Clear()
    $global:typed = @($password)
    $global:latestTag = 'v0.1.0'
    $global:latestKeys = $key2
    Use-Mode Activate '' (Join-Path $out3 'ciphera-release.key')
    Assert-Throws 'activation avant la release N refusee' 'ne distribue pas encore' { Invoke-Main }
    Assert 'activation avant la release N : aucun secret envoye' { @(Get-Sets).Count -eq 0 }
    $global:latestTag = ''
    $global:typed = @($password)
    Assert-Throws 'activation sans aucune release refusee' 'aucune release publiee' { Invoke-Main }

    # -Activate apres la release N : reconnue dans l'ensemble et distribuee.
    $global:calls.Clear()
    $global:typed = @($password)
    $global:latestTag = 'v0.2.0'
    $global:latestKeys = "$key2 $key3"
    Use-Mode Activate '' (Join-Path $out3 'ciphera-release.key')
    Invoke-Main *> $null
    Assert 'activation : cle secrete de la nouvelle cle envoyee' {
        ((Get-Sets) | Where-Object { $_.Args[2] -eq 'CIPHERA_MINISIGN_SECRET_KEY' }).Stdin -match [regex]::Escape($key3)
    }

    # -Activate d'une cle hors de l'ensemble : refusee, rien n'est envoye.
    $global:calls.Clear()
    $foreign = Join-Path $work 'foreign'
    New-Item -ItemType Directory -Path $foreign | Out-Null
    & minisign -G -p (Join-Path $foreign 'f.pub') -s (Join-Path $foreign 'f.key')
    $global:calls.Clear()
    $global:typed = @($password)
    Use-Mode Activate '' (Join-Path $foreign 'f.key')
    Assert-Throws 'activation : cle inconnue refusee' 'aucune cle de confiance' { Invoke-Main }
    Assert 'activation refusee : aucun secret envoye' { @(Get-Sets).Count -eq 0 }

    # Troisieme cle, puis quatrieme refusee.
    $global:typed = @($password, $password)
    Use-Mode Rotate (Join-Path $work 'out4')
    Invoke-Main *> $null
    $global:typed = @($password, $password)
    Use-Mode Rotate (Join-Path $work 'out5')
    Assert-Throws 'rotation : plus de 3 cles refusee' 'retirer' { Invoke-Main }
}
finally {
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item function:global:Read-Host, function:global:minisign, function:global:gh -ErrorAction SilentlyContinue
}

if ($failures -ne 0) { Write-Host "$failures echec(s)"; exit 1 }
Write-Host 'tous les tests passent'
