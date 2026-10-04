# PUBLIC TEST FIXTURES ONLY. Never install this CA or use these keys in production.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$start = [DateTimeOffset]::Parse('2020-01-01T00:00:00Z')
$end = [DateTimeOffset]::Parse('2040-01-01T00:00:00Z')
$caKey = [Security.Cryptography.ECDsa]::Create([Security.Cryptography.ECCurve+NamedCurves]::nistP256)
$caRequest = [Security.Cryptography.X509Certificates.CertificateRequest]::new('CN=G0 PUBLIC TEST CA', $caKey, [Security.Cryptography.HashAlgorithmName]::SHA256)
$caRequest.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509BasicConstraintsExtension]::new($true,$false,0,$true))
$caRequest.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509KeyUsageExtension]::new([Security.Cryptography.X509Certificates.X509KeyUsageFlags]::KeyCertSign,$true))
$ca = $caRequest.CreateSelfSigned($start,$end)
[IO.File]::WriteAllBytes((Join-Path $PSScriptRoot 'ca.der'),$ca.Export([Security.Cryptography.X509Certificates.X509ContentType]::Cert))
foreach ($kind in @('server','client')) {
    $key = [Security.Cryptography.ECDsa]::Create([Security.Cryptography.ECCurve+NamedCurves]::nistP256)
    $request = [Security.Cryptography.X509Certificates.CertificateRequest]::new("CN=$kind.g0.test",$key,[Security.Cryptography.HashAlgorithmName]::SHA256)
    $san = [Security.Cryptography.X509Certificates.SubjectAlternativeNameBuilder]::new()
    $san.AddDnsName("$kind.g0.test")
    $request.CertificateExtensions.Add($san.Build())
    $request.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509BasicConstraintsExtension]::new($false,$false,0,$true))
    $request.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509KeyUsageExtension]::new([Security.Cryptography.X509Certificates.X509KeyUsageFlags]::DigitalSignature,$true))
    $usages = [Security.Cryptography.OidCollection]::new()
    $usage = if ($kind -eq 'server') { '1.3.6.1.5.5.7.3.1' } else { '1.3.6.1.5.5.7.3.2' }
    $null = $usages.Add([Security.Cryptography.Oid]::new($usage))
    $request.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension]::new($usages,$true))
    $serial = [Security.Cryptography.RandomNumberGenerator]::GetBytes(16)
    $certificate = $request.Create($ca,$start,$end,$serial)
    [IO.File]::WriteAllBytes((Join-Path $PSScriptRoot "$kind.der"),$certificate.Export([Security.Cryptography.X509Certificates.X509ContentType]::Cert))
    [IO.File]::WriteAllBytes((Join-Path $PSScriptRoot "$kind-key.der"),$key.ExportPkcs8PrivateKey())
    $certificate.Dispose()
    $key.Dispose()
}
$ca.Dispose()
$caKey.Dispose()
