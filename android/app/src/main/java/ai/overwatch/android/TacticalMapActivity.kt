package ai.overwatch.android

import android.annotation.SuppressLint
import android.graphics.Bitmap
import android.os.Bundle
import android.webkit.WebChromeClient
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.Toast
import androidx.appcompat.app.AppCompatActivity

class TacticalMapActivity : AppCompatActivity() {

    private lateinit var webView: WebView

    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        webView = WebView(this)
        setContentView(webView)

        val hub = intent.getStringExtra(EXTRA_HUB_URL)?.trim().orEmpty()
        val mapUrl = normalizeMapUrl(hub)

        supportActionBar?.title = "Tactical Map"
        supportActionBar?.subtitle = mapUrl

        webView.settings.javaScriptEnabled = true
        webView.settings.domStorageEnabled = true
        webView.settings.useWideViewPort = true
        webView.settings.loadWithOverviewMode = true

        webView.webChromeClient = WebChromeClient()
        webView.webViewClient = object : WebViewClient() {
            override fun onPageStarted(view: WebView?, url: String?, favicon: Bitmap?) {
                super.onPageStarted(view, url, favicon)
            }

            override fun shouldOverrideUrlLoading(view: WebView?, request: WebResourceRequest?): Boolean {
                return false
            }
        }

        runCatching { webView.loadUrl(mapUrl) }
            .onFailure { Toast.makeText(this, "Failed to load map: ${it.message}", Toast.LENGTH_LONG).show() }
    }

    override fun onBackPressed() {
        if (::webView.isInitialized && webView.canGoBack()) {
            webView.goBack()
        } else {
            super.onBackPressed()
        }
    }

    private fun normalizeMapUrl(hubUrl: String): String {
        if (hubUrl.startsWith("http://") || hubUrl.startsWith("https://")) {
            val base = hubUrl.trimEnd('/')
            return "$base/"
        }
        return "http://10.0.0.5:8789/"
    }

    companion object {
        const val EXTRA_HUB_URL = "extra_hub_url"
    }
}
