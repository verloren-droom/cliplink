package com.benfach.cliplink

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.io.File
import java.security.KeyStore
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

object AndroidKeyStoreBridge {
    private const val KEYSTORE_PROVIDER = "AndroidKeyStore"
    private const val KEY_ALIAS = "com.benfach.cliplink.local_data"
    private const val TRANSFORMATION = "AES/GCM/NoPadding"
    private const val KEY_SIZE_BITS = 256
    private const val NONCE_BYTES = 12
    private const val TAG_BITS = 128
    private const val LOCAL_DATA_KEY_BYTES = 32
    private const val WRAPPED_KEY_DIRECTORY = "cliplink"
    private const val WRAPPED_KEY_FILE_NAME = "local-data.key"

    fun loadOrCreateLocalDataKey(context: Context): ByteArray {
        val wrappedKeyFile = File(
            File(context.applicationContext.noBackupFilesDir, WRAPPED_KEY_DIRECTORY),
            WRAPPED_KEY_FILE_NAME,
        )
        val wrappingKey = loadOrCreateWrappingKey()
        if (wrappedKeyFile.exists()) {
            return unwrapLocalDataKey(wrappingKey, wrappedKeyFile.readBytes())
        }

        val localDataKey = ByteArray(LOCAL_DATA_KEY_BYTES)
        SecureRandom().nextBytes(localDataKey)
        persistWrappedLocalDataKey(wrappedKeyFile, wrapLocalDataKey(wrappingKey, localDataKey))
        return localDataKey
    }

    private fun loadOrCreateWrappingKey(): SecretKey {
        val keyStore = KeyStore.getInstance(KEYSTORE_PROVIDER).apply { load(null) }
        val existing = keyStore.getKey(KEY_ALIAS, null)
        if (existing is SecretKey) {
            return existing
        }

        val keyGenerator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE_PROVIDER)
        val spec = KeyGenParameterSpec.Builder(
            KEY_ALIAS,
            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
        )
            .setKeySize(KEY_SIZE_BITS)
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setRandomizedEncryptionRequired(true)
            .build()
        keyGenerator.init(spec)
        return keyGenerator.generateKey()
    }

    private fun wrapLocalDataKey(wrappingKey: SecretKey, localDataKey: ByteArray): ByteArray {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, wrappingKey)
        val ciphertext = cipher.doFinal(localDataKey)
        return cipher.iv + ciphertext
    }

    private fun unwrapLocalDataKey(wrappingKey: SecretKey, wrapped: ByteArray): ByteArray {
        require(wrapped.size > NONCE_BYTES) { "Wrapped local-data key is invalid." }
        val nonce = wrapped.copyOfRange(0, NONCE_BYTES)
        val ciphertext = wrapped.copyOfRange(NONCE_BYTES, wrapped.size)
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.DECRYPT_MODE, wrappingKey, GCMParameterSpec(TAG_BITS, nonce))
        val plaintext = cipher.doFinal(ciphertext)
        require(plaintext.size == LOCAL_DATA_KEY_BYTES) {
            "Unwrapped local-data key has an unexpected length."
        }
        return plaintext
    }

    private fun persistWrappedLocalDataKey(target: File, wrapped: ByteArray) {
        val parent = checkNotNull(target.parentFile) { "Wrapped key file must have a parent directory." }
        parent.mkdirs()
        val temp = File(parent, "${target.name}.tmp")
        temp.writeBytes(wrapped)
        if (target.exists() && !target.delete()) {
            temp.delete()
            error("Failed to replace the wrapped local-data key file.")
        }
        if (!temp.renameTo(target)) {
            temp.delete()
            error("Failed to move the wrapped local-data key file into place.")
        }
    }
}
