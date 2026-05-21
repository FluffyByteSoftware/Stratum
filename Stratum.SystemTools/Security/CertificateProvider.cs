/*
 * (CertificateProvider.cs)
 *------------------------------------------------------------
 * Created - 5/20/2026 9:32:33 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System.Security.Cryptography;
using System.Security.Cryptography.X509Certificates;

namespace Stratum.SystemTools.Security;

/// <summary>
/// Manages self-signed X.509 certificates with automatic loading, creation, and persistence.
/// 
/// Note: DOES NOT USE THE DISKMANAGER TO PERFORM CERTIFICATE WRITES OR READS.
/// This is because the certificate files are expected to be used by external tools (e.g. OpenSSL) 
/// and must be written in a standard format that those tools can read, which may not align with 
/// the abstractions provided by the DiskManager. Additionally, the certificate files are typically 
/// small and infrequently accessed, so the performance benefits of using the DiskManager's 
/// caching and batching features are minimal in this context.
/// </summary>
public static class CertificateProvider
{
    private const string SubjectName = "CN-Stratum";
    private const string PfxRelativePath = "certs/server.pfx";
    private const string CerRelativePath = "certs/server.cer";
    private static readonly TimeSpan Lifetime = TimeSpan.FromDays(365);

    /// <summary>
    /// Loads an existing X.509 certificate from the specified path if valid, or creates and persists a new certificate.
    /// </summary>
    /// <remarks>If an existing PFX certificate file is found but has expired, it is disposed and a new
    /// certificate is generated.</remarks>
    /// <param name="rootPath">The root directory path where the certificate files are stored.</param>
    /// <returns>A valid <see cref="X509Certificate2"/> instance.</returns>
    public static X509Certificate2 LoadOrCreate(string rootPath)
    {
        var fullRoot = Path.GetFullPath(rootPath);
        var pfxPath = Path.Combine(fullRoot, PfxRelativePath);
        var cerPath = Path.Combine(fullRoot, CerRelativePath);

        if (File.Exists(pfxPath))
        {
            var existing = X509CertificateLoader.LoadPkcs12FromFile(
                pfxPath,
                password: null,
                keyStorageFlags: X509KeyStorageFlags.Exportable);

            if (existing.NotAfter > DateTime.UtcNow)
                return existing;

            existing.Dispose();
        }

        return GenerateAndPersist(pfxPath, cerPath);
    }

    private static X509Certificate2 GenerateAndPersist(
        string pfxPath,
        string cerPath)
    {
        using var ecdsa = ECDsa.Create(ECCurve.NamedCurves.nistP256);

        var request = new CertificateRequest(
            SubjectName,
            ecdsa,
            HashAlgorithmName.SHA256);

        var notBefore = DateTimeOffset.UtcNow.AddMinutes(-5);
        var notAfter = notBefore.Add(Lifetime);

        using var unbound = request.CreateSelfSigned(notBefore, notAfter);

        var pfxBytes = unbound.Export(X509ContentType.Pfx);
        var cerBytes = unbound.Export(X509ContentType.Cert);

        WriteAtomic(pfxPath, pfxBytes);
        WriteAtomic(cerPath, cerBytes);

        return X509CertificateLoader.LoadPkcs12(
            pfxBytes,
            password: null,
            keyStorageFlags: X509KeyStorageFlags.Exportable);
    }

    // Self writing to a temp file and then replacing the target file is atomic on most
    // platforms, and ensures that we don't end up with a corrupted file if the
    // process is interrupted during writing.
    private static void WriteAtomic(string fullPath, byte[] data)
    {
        var dir = Path.GetDirectoryName(fullPath);
        
        if (!string.IsNullOrEmpty(dir))
            Directory.CreateDirectory(dir);

        var tmpPath = fullPath + ".tmp";

        using (var fs = new FileStream(
            tmpPath,
            FileMode.Create,
            FileAccess.Write,
            FileShare.None))
        {
            fs.Write(data, 0, data.Length);
            fs.Flush(true);
        }

        if (File.Exists(fullPath))
            File.Replace(tmpPath, fullPath, destinationBackupFileName: null);
        else
            File.Move(tmpPath, fullPath);        
    }
}



/*
 *------------------------------------------------------------
 * (CertificateProvider.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */