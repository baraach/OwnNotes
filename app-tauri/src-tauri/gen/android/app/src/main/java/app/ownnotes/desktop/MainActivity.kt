package app.ownnotes.desktop

import android.os.Bundle
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {
  // Read by the page at startup, because the first insets can arrive before it loads.
  @Volatile private var insets = ""

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
  }

  override fun onWebViewCreate(webView: WebView) {
    webView.addJavascriptInterface(Bridge(), "OwnNotesAndroid")
    ViewCompat.setOnApplyWindowInsetsListener(webView) { view, windowInsets ->
      val density = resources.displayMetrics.density
      val bars = windowInsets.getInsets(
        WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
      )
      val keyboard = windowInsets.getInsets(WindowInsetsCompat.Type.ime())
      val top = (bars.top / density).toInt()
      val bottom = (bars.bottom / density).toInt()
      val typing = (keyboard.bottom / density).toInt()
      insets = "$top,$bottom,$typing"
      (view as WebView).evaluateJavascript(
        "window.__applyAndroidInsets && window.__applyAndroidInsets($top,$bottom,$typing)",
        null
      )
      windowInsets
    }
  }

  private inner class Bridge {
    @JavascriptInterface
    fun insets(): String = this@MainActivity.insets

    /** Dark status-bar icons for a light app theme, and the other way round. */
    @JavascriptInterface
    fun setLightBars(light: Boolean) {
      runOnUiThread {
        val controller = WindowCompat.getInsetsController(window, window.decorView)
        controller.isAppearanceLightStatusBars = light
        controller.isAppearanceLightNavigationBars = light
      }
    }
  }
}
