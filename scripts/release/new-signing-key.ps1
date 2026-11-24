#Requires -Version 7.0
<#
.SYNOPSIS
  Cree, fait tourner ou active la cle minisign de production de Ciphera.

.DESCRIPTION
  A lancer par la personne qui garde la cle, dans son propre terminal
  (PowerShell 7 : Windows, Linux ou macOS). Aucun agent ni la CI ne lance
  ce script. Procedure complete : docs/security/release-signing.md.

  Trois modes :

  (defaut) premiere cle : install/ciphera.pub porte encore la cle de test.
    Demande deux fois le mot de passe (jamais affiche, jamais en argument),
    genere la paire HORS du depot (-OutDir), la verifie (signature d'essai),
    ecrit SEULEMENT la cle publique dans install/ciphera.pub (remplace la
    cle de test), puis envoie la cle secrete (chiffree) et le mot de passe
    dans l'environnement GitHub `release` (CIPHERA_MINISIGN_SECRET_KEY,
    CIPHERA_MINISIGN_PASSWORD) par `gh secret set`, valeurs sur l'entree
    standard.

  -Rotate : nouvelle paire AJOUTEE aux cles de confiance de
    install/ciphera.pub (l'ancienne reste) ; rien n'est envoye a GitHub. La
    release suivante, encore signee par l'ancienne cle, distribue les deux
    cles ; ensuite -Activate.

  -Activate -SecretKeyFile <fichier> : envoie une cle secrete existante (et
    son mot de passe) dans l'environnement `release`, apres avoir verifie
    qu'elle correspond a une cle de install/ciphera.pub. Sert aussi a
    reprendre un envoi interrompu. Pendant une rotation, refuse une cle que
    la derniere release publiee ne distribue pas encore (release N).

  -Verify -SecretKeyFile <fichier> : verifie seulement qu'une cle secrete
    (copie hors ligne) et son mot de passe correspondent a une cle de
    install/ciphera.pub. Aucun appel a GitHub, rien n'est envoye.

  Avant tout envoi, l'environnement `release` doit exiger un relecteur et
  n'accepter que les tags v*.

  Les fichiers temporaires (signature d'essai) sont reecrits puis supprimes.

.PARAMETER OutDir
  Dossier (hors depot) ou la cle secrete chiffree est ecrite pour etre
  sauvegardee hors ligne. Defaut : ~/ciphera-release-key-AAAAMMJJHHMMSS.

.PARAMETER Repo
  Depot GitHub (defaut : ciphera-dev/deploy-hub).
#>
[CmdletBinding(DefaultParameterSetName = 'Init')]
param(
    [Parameter(ParameterSetName = 'Init')]
    [Parameter(ParameterSetName = 'Rotate')]
    [string]$OutDir = (Join-Path $HOME ("ciphera-release-key-" + (Get-Date -Format 'yyyyMMddHHmmss'))),
    [string]$Repo = 'ciphera-dev/deploy-hub',
    [Parameter(ParameterSetName = 'Rotate', Mandatory)]
    [switch]$Rotate,
    [Parameter(ParameterSetName = 'Activate', Mandatory)]
    [switch]$Activate,
    [Parameter(ParameterSetName = 'Verify', Mandatory)]
    [switch]$Verify,
    [Parameter(ParameterSetName = 'Activate', Mandatory)]
    [Parameter(ParameterSetName = 'Verify', Mandatory)]
    [string]$SecretKeyFile
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$SecretKeyName = 'CIPHERA_MINISIGN_SECRET_KEY'
$PasswordName = 'CIPHERA_MINISIGN_PASSWORD'
$Environment = 'release'
$MinPasswordLength = 16
$MaxTrustedKeys = 3
$ScriptName = 'scripts/release/new-signing-key.ps1'

function Write-Step([string]$Message) {
    Write-Host "==> $Message"
}

function Get-RepoRoot {
    (Resolve-Path (Join-Path $PSScriptRoot '..' '..')).Path
}

# Chemin absolu resolu depuis le dossier courant de PowerShell ($PWD), pas
# celui du processus.
function Resolve-UserPath([string]$Path) {
    $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Path)
}

# Vrai si <Path> est <Root> ou un de ses sous-dossiers.
function Test-PathInside([string]$Path, [string]$Root) {
    $sep = [IO.Path]::DirectorySeparatorChar
    $p = (Resolve-UserPath $Path).TrimEnd($sep, '/') + $sep
    $r = (Resolve-UserPath $Root).TrimEnd($sep, '/') + $sep
    $cmp = if ($IsWindows -or $IsMacOS) { [StringComparison]::OrdinalIgnoreCase } else { [StringComparison]::Ordinal }
    $p.StartsWith($r, $cmp)
}

# Cles d'un fichier minisign (une ou plusieurs) : lignes ni vides ni
# commentaires, chacune validee (RW..., 56 caracteres). Meme lecture que
# install/release-script.sh et ciphera-cli updater.
function Get-PublicKeyLines([string]$PubFile) {
    $keys = @(Get-Content -LiteralPath $PubFile | ForEach-Object { $_.Trim() } |
            Where-Object { $_ -ne '' -and -not $_.StartsWith('untrusted comment:') })
    if ($keys.Count -eq 0) { throw "aucune cle publique dans $PubFile" }
    foreach ($k in $keys) {
        if ($k -cnotmatch '^RW[A-Za-z0-9+/=]{54}$') { throw "cle publique invalide dans $PubFile" }
    }
    , $keys
}

function Get-PublicKeyLine([string]$PubFile) {
    $keys = Get-PublicKeyLines $PubFile
    if ($keys.Count -ne 1) { throw "une seule cle attendue dans $PubFile" }
    $keys[0]
}

function Test-Tool([string]$Name, [string]$InstallHint) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "$Name introuvable. Installation : $InstallHint"
    }
}

function Read-Passphrase([switch]$Confirm) {
    $a = Read-Host -AsSecureString "Mot de passe de la cle (au moins $MinPasswordLength caracteres)"
    $pa = [Net.NetworkCredential]::new('', $a).Password
    if ($Confirm) {
        $b = Read-Host -AsSecureString 'Mot de passe (confirmation)'
        if ($pa -cne [Net.NetworkCredential]::new('', $b).Password) { throw 'les deux mots de passe different' }
    }
    if ($pa.Length -lt $MinPasswordLength) { throw "mot de passe trop court (minimum $MinPasswordLength caracteres)" }
    if ($pa -match '[\r\n]') { throw 'le mot de passe ne doit pas contenir de retour a la ligne' }
    $pa
}

# Reecrit un fichier avec des octets aleatoires puis le supprime. Sur SSD ou
# systeme de fichiers journalise, l'effacement n'est pas garanti : seuls des
# fichiers sans secret en clair passent par ici (la cle est chiffree).
function Remove-FileSecurely([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return }
    $len = (Get-Item -LiteralPath $Path).Length
    if ($len -gt 0) {
        $bytes = [byte[]]::new($len)
        [Security.Cryptography.RandomNumberGenerator]::Fill($bytes)
        [IO.File]::WriteAllBytes($Path, $bytes)
    }
    Remove-Item -LiteralPath $Path -Force
}

# Lance <Command> avec <InputText> sur l'entree standard ; sortie gardee
# (affichee seulement en cas d'echec, elle ne contient aucun secret).
function Invoke-WithStdin([string]$InputText, [string]$Command, [string[]]$Arguments) {
    $output = $InputText | & $Command @Arguments 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "$Command a echoue (code $LASTEXITCODE) : $($output -join ' ')"
    }
}

function Test-SecretExists([string]$Repo) {
    $names = gh secret list --env $Environment --repo $Repo --json name --jq '.[].name'
    if ($LASTEXITCODE -ne 0) { throw "gh secret list a echoue (environnement $Environment de $Repo)" }
    @($names) -contains $SecretKeyName
}

function Assert-GitHubReady([string]$Repo) {
    Write-Step "verification de gh et de l'environnement $Environment de $Repo"
    gh auth status *> $null
    if ($LASTEXITCODE -ne 0) { throw 'gh non connecte : gh auth login' }
    $envJson = gh api "repos/$Repo/environments/$Environment"
    if ($LASTEXITCODE -ne 0) { throw "environnement GitHub '$Environment' introuvable dans $Repo (docs/operations/release.md, « Verifier l'environnement release »)" }
    $envInfo = ($envJson -join "`n") | ConvertFrom-Json
    $reviewers = @($envInfo.protection_rules | Where-Object { $_.type -eq 'required_reviewers' } |
            ForEach-Object { @($_.reviewers) })
    $policy = $envInfo.deployment_branch_policy
    $policiesJson = gh api "repos/$Repo/environments/$Environment/deployment-branch-policies"
    if ($LASTEXITCODE -ne 0) { throw "regles de deploiement de '$Environment' illisibles" }
    $policies = @((($policiesJson -join "`n") | ConvertFrom-Json).branch_policies)
    $problems = @()
    if ($reviewers.Count -eq 0) { $problems += 'aucun relecteur obligatoire (Required reviewers)' }
    if ($null -eq $policy -or -not $policy.custom_branch_policies) {
        $problems += 'deploiements non limites (Deployment branches and tags : Selected branches and tags)'
    }
    elseif (@($policies | Where-Object { $_.type -eq 'tag' -and $_.name -eq 'v*' }).Count -ne 1 -or
        @($policies | Where-Object { -not ($_.type -eq 'tag' -and $_.name -eq 'v*') }).Count -ne 0) {
        $problems += 'les regles de deploiement doivent etre exactement : tag v* (aucune branche)'
    }
    if ($problems.Count) {
        throw "environnement '$Environment' non protege : $($problems -join ' ; '). Voir docs/operations/release.md, « Verifier l'environnement release »."
    }
}

# Pendant une rotation : la cle a activer doit deja etre distribuee par la
# derniere release publiee (release N), sinon les serveurs existants
# refuseraient la prochaine release.
function Assert-KeyPublished([string]$Repo, [string]$Key) {
    $tag = gh release view --repo $Repo --json tagName --jq .tagName
    if ($LASTEXITCODE -ne 0 -or -not $tag) { throw "aucune release publiee ne distribue encore la cle $Key : publier d'abord la release N (docs/security/release-signing.md, Rotation)" }
    $script = gh release download $tag --repo $Repo --pattern install.sh --output -
    if ($LASTEXITCODE -ne 0) { throw "install.sh de la release $tag illisible" }
    $line = @($script | Where-Object { $_ -like 'CIPHERA_PUBKEYS=*' }) -join ''
    if (-not $line.Contains($Key)) {
        throw "la derniere release publiee ($tag) ne distribue pas encore la cle $Key : publier d'abord la release N avec install/ciphera.pub a deux cles, puis activer (docs/security/release-signing.md, Rotation)"
    }
}

# Signe un fichier d'essai avec <SecretKey> et renvoie la cle de <Keys> qui
# verifie la signature (erreur si aucune : paire ou mot de passe faux).
function Find-SigningKey([string]$SecretKey, [string]$Password, [string[]]$Keys) {
    $tmp = Join-Path ([IO.Path]::GetTempPath()) ('ciphera-key-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        $probe = Join-Path $tmp 'probe.txt'
        Set-Content -LiteralPath $probe -Value 'ciphera release key probe' -NoNewline
        Invoke-WithStdin $Password 'minisign' @('-S', '-s', $SecretKey, '-m', $probe, '-t', 'probe')
        foreach ($k in $Keys) {
            & minisign -V -q -P $k -m $probe *> $null
            if ($LASTEXITCODE -eq 0) { return $k }
        }
        throw 'la signature d''essai ne se verifie avec aucune cle de confiance : rien n''est envoye'
    }
    finally {
        Get-ChildItem -LiteralPath $tmp -File -ErrorAction SilentlyContinue | ForEach-Object { Remove-FileSecurely $_.FullName }
        Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }
}

# Genere une paire chiffree dans <Dir> et la verifie. En cas d'echec, les
# fichiers generees sont effaces : rien n'a quitte le poste.
function New-KeyPair([string]$Dir, [string]$Password, [string]$TestKey) {
    New-Item -ItemType Directory -Path $Dir -Force | Out-Null
    $sec = Join-Path $Dir 'ciphera-release.key'
    $pub = Join-Path $Dir 'ciphera-release.pub'
    try {
        Write-Step "generation de la paire minisign dans $Dir"
        Invoke-WithStdin "$Password`n$Password" 'minisign' @('-G', '-p', $pub, '-s', $sec)
        Write-Step 'verification de la paire et du mot de passe (signature d''essai)'
        $key = Get-PublicKeyLine $pub
        if ($key -ceq $TestKey) { throw 'cle generee identique a la cle de test' }
        [void](Find-SigningKey $sec $Password @($key))
    }
    catch {
        Remove-FileSecurely $sec
        Remove-FileSecurely $pub
        throw "$($_.Exception.Message) (paire inutilisable effacee de $Dir, rien n'a ete envoye ni ecrit dans le depot : relancer)"
    }
    @{ Secret = $sec; Public = $pub; Key = $key }
}

function Write-PubFile([string]$Path, [string[]]$Texts) {
    $all = ($Texts | ForEach-Object { ($_ -replace "`r`n", "`n").TrimEnd("`n") }) -join "`n"
    [IO.File]::WriteAllText($Path, $all + "`n", [Text.UTF8Encoding]::new($false))
}

# Envoie la cle secrete puis le mot de passe. Un echec dit exactement ce qui
# est parti et comment reprendre.
function Publish-Secrets([string]$Repo, [string]$SecretKey, [string]$Password) {
    $sent = [Collections.Generic.List[string]]::new()
    try {
        Write-Step "envoi des secrets $SecretKeyName et $PasswordName (environnement $Environment)"
        $secretText = (Get-Content -LiteralPath $SecretKey -Raw) -replace "`r`n", "`n"
        Invoke-WithStdin $secretText 'gh' @('secret', 'set', $SecretKeyName, '--env', $Environment, '--repo', $Repo)
        $sent.Add($SecretKeyName)
        Invoke-WithStdin $Password 'gh' @('secret', 'set', $PasswordName, '--env', $Environment, '--repo', $Repo)
        $sent.Add($PasswordName)
    }
    catch {
        $done = if ($sent.Count) { $sent -join ', ' } else { 'aucun' }
        throw ("envoi interrompu : $($_.Exception.Message)`n" +
            "  secrets deja envoyes : $done (tant que les deux ne vont pas ensemble, sign-release echoue : aucune release ne peut etre signee).`n" +
            "  La cle reste dans $SecretKey. Reprendre : pwsh $ScriptName -Activate -SecretKeyFile '$SecretKey'")
    }
    finally {
        $secretText = $null
    }
}

function Write-BackupSteps([string]$SecretKey) {
    Write-Host "  - Copier $SecretKey sur DEUX supports hors ligne (cle USB chiffree, coffre),"
    Write-Host '    et le mot de passe dans un gestionnaire de mots de passe (jamais a cote du fichier).'
    Write-Host "  - Supprimer ensuite la copie locale : Remove-Item -Recurse '$(Split-Path -Parent $SecretKey)'"
}

function Invoke-Init([string]$Root, [string]$PubTarget, [string]$TestKey) {
    $current = Get-PublicKeyLines $PubTarget
    if ($current.Count -ne 1 -or $current[0] -cne $TestKey) {
        throw "install/ciphera.pub porte deja une cle de production : rotation = -Rotate puis -Activate (docs/security/release-signing.md)"
    }
    $out = Resolve-UserPath $OutDir
    if (Test-PathInside $out $Root) { throw "OutDir doit etre hors du depot : $out" }
    if ((Test-Path -LiteralPath $out) -and @(Get-ChildItem -LiteralPath $out -Force).Count -gt 0) {
        throw "OutDir existe et n'est pas vide : $out (choisir un autre -OutDir)"
    }
    Assert-GitHubReady $Repo
    if (Test-SecretExists $Repo) {
        throw ("le secret $SecretKeyName existe deja. Si sa cle est dans install/ciphera.pub (commit), la reprendre avec -Activate -SecretKeyFile <fichier>. " +
            "Si install/ciphera.pub est revenu a la cle de test (cle perdue ou abandonnee), supprimer d'abord les secrets : " +
            "gh secret delete $SecretKeyName --env $Environment --repo $Repo ; gh secret delete $PasswordName --env $Environment --repo $Repo")
    }

    $password = Read-Passphrase -Confirm
    try {
        $pair = New-KeyPair $out $password $TestKey
        Write-Step 'ecriture de install/ciphera.pub (cle publique seulement, remplace la cle de test)'
        Write-PubFile $PubTarget @(Get-Content -LiteralPath $pair.Public -Raw)
        Publish-Secrets $Repo $pair.Secret $password
    }
    finally {
        $password = $null
        [GC]::Collect()
    }

    Write-Host ''
    Write-Host "Cle de production creee : $((Get-Content -LiteralPath $pair.Public -TotalCount 1))"
    Write-Host 'A faire maintenant :'
    Write-BackupSteps $pair.Secret
    Write-Host '  - Commiter install/ciphera.pub (seule la cle publique a change) et suivre docs/operations/release.md.'
}

function Invoke-Rotate([string]$Root, [string]$PubTarget, [string]$TestKey) {
    $current = Get-PublicKeyLines $PubTarget
    if ($current -ccontains $TestKey) { throw 'install/ciphera.pub contient la cle de test : premiere cle = mode par defaut' }
    if ($current.Count -ge $MaxTrustedKeys) {
        throw "install/ciphera.pub porte deja $($current.Count) cles : retirer d'abord une ancienne cle (docs/security/release-signing.md)"
    }
    $out = Resolve-UserPath $OutDir
    if (Test-PathInside $out $Root) { throw "OutDir doit etre hors du depot : $out" }
    if ((Test-Path -LiteralPath $out) -and @(Get-ChildItem -LiteralPath $out -Force).Count -gt 0) {
        throw "OutDir existe et n'est pas vide : $out (choisir un autre -OutDir)"
    }

    $password = Read-Passphrase -Confirm
    try {
        $pair = New-KeyPair $out $password $TestKey
    }
    finally {
        $password = $null
        [GC]::Collect()
    }
    Write-Step 'ajout de la nouvelle cle publique a install/ciphera.pub (les anciennes restent)'
    Write-PubFile $PubTarget @((Get-Content -LiteralPath $PubTarget -Raw), (Get-Content -LiteralPath $pair.Public -Raw))

    Write-Host ''
    Write-Host "Nouvelle cle creee (pas encore active) : $((Get-Content -LiteralPath $pair.Public -TotalCount 1))"
    Write-Host 'A faire maintenant (docs/security/release-signing.md, Rotation) :'
    Write-BackupSteps $pair.Secret
    Write-Host '  - Commiter install/ciphera.pub et publier une release N, encore signee par la cle actuelle.'
    Write-Host "  - Puis activer la nouvelle cle : pwsh $ScriptName -Activate -SecretKeyFile '$($pair.Secret)'"
}

function Invoke-Activate([string]$PubTarget, [string]$TestKey) {
    $file = Resolve-UserPath $SecretKeyFile
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "cle secrete introuvable : $file" }
    $all = Get-PublicKeyLines $PubTarget
    $trusted = @($all | Where-Object { $_ -cne $TestKey })
    if ($trusted.Count -eq 0) { throw 'install/ciphera.pub ne porte aucune cle de production' }
    Assert-GitHubReady $Repo

    $password = Read-Passphrase
    try {
        Write-Step 'verification de la cle et du mot de passe (signature d''essai)'
        $key = Find-SigningKey $file $password $trusted
        Write-Step "cle reconnue : $key"
        if ($trusted.Count -gt 1) { Assert-KeyPublished $Repo $key }
        Publish-Secrets $Repo $file $password
    }
    finally {
        $password = $null
        [GC]::Collect()
    }
    Write-Host ''
    Write-Host "Cle active pour les prochaines releases : $key"
    Write-Host '  - La cle ne sert plus sur ce poste : supprimer la copie locale une fois les sauvegardes hors ligne verifiees.'
}

function Invoke-Verify([string]$PubTarget) {
    $file = Resolve-UserPath $SecretKeyFile
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "cle secrete introuvable : $file" }
    $trusted = Get-PublicKeyLines $PubTarget
    $password = Read-Passphrase
    try {
        Write-Step 'verification de la cle et du mot de passe (signature d''essai, aucun envoi)'
        $key = Find-SigningKey $file $password $trusted
    }
    finally {
        $password = $null
        [GC]::Collect()
    }
    Write-Host "Cle lisible, mot de passe correct, cle de confiance : $key"
}

function Invoke-Main {
    $root = Get-RepoRoot
    $pubTarget = Join-Path $root 'install' 'ciphera.pub'
    $testKey = Get-PublicKeyLine (Join-Path $root 'install' 'tests' 'ciphera-test.pub')

    Test-Tool 'minisign' 'Windows : winget install jedisct1.minisign ; macOS : brew install minisign ; Debian/Ubuntu : apt install minisign ; Fedora : dnf install minisign'
    if (-not $Rotate -and -not $Verify) {
        Test-Tool 'gh' 'https://cli.github.com (Windows : winget install GitHub.cli), puis gh auth login'
    }
    if ($Verify) { Invoke-Verify $pubTarget }
    elseif ($Activate) { Invoke-Activate $pubTarget $testKey }
    elseif ($Rotate) { Invoke-Rotate $root $pubTarget $testKey }
    else { Invoke-Init $root $pubTarget $testKey }
}

if ($MyInvocation.InvocationName -ne '.') {
    try {
        Invoke-Main
    }
    catch {
        Write-Host "erreur : $($_.Exception.Message)" -ForegroundColor Red
        exit 1
    }
}
