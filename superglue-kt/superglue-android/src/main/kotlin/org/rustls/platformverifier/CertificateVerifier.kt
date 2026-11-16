package org.rustls.platformverifier

import android.content.Context
import android.net.http.X509TrustManagerExtensions
import java.io.ByteArrayInputStream
import java.security.KeyStore
import java.security.cert.CertificateException
import java.security.cert.CertificateExpiredException
import java.security.cert.CertificateFactory
import java.security.cert.CertificateNotYetValidException
import java.security.cert.CertificateParsingException
import java.security.cert.CertificateRevokedException
import java.security.cert.X509Certificate
import java.util.Date
import javax.net.ssl.TrustManagerFactory
import javax.net.ssl.X509TrustManager

/**
 * JNI target for `rustls-platform-verifier`.
 *
 * The stock checker runs a second PKIX OCSP pass after [X509TrustManagerExtensions.checkServerTrusted]
 * has already accepted the chain. On this platform that pass throws and the caller reports
 * [Status.REVOKED]. Android's own check already enforces revocation, including
 * [CertificateRevokedException].
 */
internal object CertificateVerifier {
    private const val OK = 0
    private const val UNAVAILABLE = 1
    private const val EXPIRED = 2
    private const val UNKNOWN_CERT = 3
    private const val REVOKED = 4
    private const val INVALID_ENCODING = 5
    private const val INVALID_EXTENSION = 6

    @JvmStatic
    private fun verifyCertificateChain(
        @Suppress("UNUSED_PARAMETER") context: Context,
        serverName: String,
        authType: String,
        allowedEkus: Array<String>?,
        @Suppress("UNUSED_PARAMETER") ocspResponse: ByteArray?,
        nowMillis: Long,
        encodedChain: Array<ByteArray>?,
    ): VerificationResult {
        if (encodedChain.isNullOrEmpty()) {
            return VerificationResult(INVALID_ENCODING, "empty certificate chain")
        }
        val certificates =
            try {
                decodeChain(encodedChain)
            } catch (error: CertificateException) {
                return VerificationResult(INVALID_ENCODING, error.toString())
            }
        val leaf = certificates[0]
        try {
            leaf.checkValidity(Date(nowMillis))
        } catch (_: CertificateExpiredException) {
            return VerificationResult(EXPIRED, null)
        } catch (_: CertificateNotYetValidException) {
            return VerificationResult(EXPIRED, null)
        }
        if (!hasAllowedEku(leaf, allowedEkus)) {
            return VerificationResult(INVALID_EXTENSION, null)
        }
        val trustManager = systemTrustManager() ?: return VerificationResult(UNAVAILABLE, null)
        return try {
            X509TrustManagerExtensions(trustManager).checkServerTrusted(
                certificates.toTypedArray(),
                authType,
                serverName,
            )
            VerificationResult(OK, null)
        } catch (error: CertificateRevokedException) {
            VerificationResult(REVOKED, error.toString())
        } catch (error: CertificateException) {
            VerificationResult(UNKNOWN_CERT, error.toString())
        }
    }

    private fun decodeChain(encodedChain: Array<ByteArray>): List<X509Certificate> {
        val factory = CertificateFactory.getInstance("X.509")
        return encodedChain.map { encoded ->
            factory.generateCertificate(ByteArrayInputStream(encoded)) as X509Certificate
        }
    }

    private fun hasAllowedEku(
        leaf: X509Certificate,
        allowedEkus: Array<String>?,
    ): Boolean {
        if (allowedEkus.isNullOrEmpty()) {
            return true
        }
        val present =
            try {
                leaf.extendedKeyUsage
            } catch (_: CertificateParsingException) {
                return false
            } ?: return true
        if (present.isEmpty()) {
            return true
        }
        return present.any { it in allowedEkus }
    }

    private fun systemTrustManager(): X509TrustManager? {
        return try {
            val factory = TrustManagerFactory.getInstance(TrustManagerFactory.getDefaultAlgorithm())
            factory.init(null as KeyStore?)
            factory.trustManagers.filterIsInstance<X509TrustManager>().firstOrNull()
        } catch (_: RuntimeException) {
            null
        }
    }
}

internal class VerificationResult(
    @JvmField val code: Int,
    @JvmField val message: String?,
)
