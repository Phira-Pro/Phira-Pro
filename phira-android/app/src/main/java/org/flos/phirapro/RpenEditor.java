package org.flos.phirapro;

import android.app.Activity;
import android.content.Intent;
import android.graphics.Color;
import android.net.Uri;
import android.view.View;
import android.view.ViewGroup;
import android.webkit.ValueCallback;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceRequest;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.webkit.JavascriptInterface;
import android.webkit.WebResourceError;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.view.Gravity;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.TextView;
import android.widget.FrameLayout;
import java.util.concurrent.atomic.AtomicBoolean;

/** Editor-only overlay. Files are selected by the system picker. */
final class RpenEditor {
    private final RpenFilePicker picker;
    private final Activity activity;
    private final FrameLayout root;
    private final WebView web;
    private final String origin;
    private final String address;
    private final LinearLayout loading;
    private final TextView loadingText;
    private final Button retry;
    private final Button safe;
    private final Handler handler = new Handler(Looper.getMainLooper());
    private final AtomicBoolean bootExit = new AtomicBoolean();
    private boolean booting = true;
    private boolean destroyed;
    private String bootStage = "";
    private final Runnable timeout = () -> failBoot("制谱器加载超时：" + bootStage);

    RpenEditor(Activity activity, String address, boolean softwareRendering) {
        this.activity=activity;
        picker=new RpenFilePicker(activity);
        this.address=address;
        Uri uri=Uri.parse(address);
        if (!"http".equals(uri.getScheme()) || !"127.0.0.1".equals(uri.getHost())) throw new IllegalArgumentException("Invalid editor address");
        origin="http://127.0.0.1:"+uri.getPort()+"/";
        root=new FrameLayout(activity);root.setBackgroundColor(Color.rgb(23,29,38));
        web=new WebView(activity);
        WebSettings settings=web.getSettings();settings.setJavaScriptEnabled(true);settings.setDomStorageEnabled(true);
        settings.setAllowFileAccess(false);settings.setAllowContentAccess(true);settings.setMediaPlaybackRequiresUserGesture(false);
        settings.setBuiltInZoomControls(false);settings.setDisplayZoomControls(false);settings.setSupportMultipleWindows(false);
        web.setBackgroundColor(Color.rgb(23,29,38));
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            root.setForceDarkAllowed(false);
            web.setForceDarkAllowed(false);
            // RPEN already provides its own theme. The app targets Android 12 or lower.
            settings.setForceDark(WebSettings.FORCE_DARK_OFF);
        }
        if (softwareRendering) web.setLayerType(View.LAYER_TYPE_SOFTWARE, null);
        web.setWebViewClient(new WebViewClient(){
            @Override public void onPageStarted(WebView view,String url,android.graphics.Bitmap icon){startBoot();}
            @Override public void onPageFinished(WebView view,String url){
                if(!booting||!url.equals(view.getUrl())||!url.startsWith(origin))return;
                // Classic syntax: this can still run when the module entry cannot parse.
                view.evaluateJavascript("typeof structuredClone === 'function' && typeof CSSStyleSheet !== 'undefined' && typeof CSSStyleSheet.prototype.replaceSync === 'function'", supported -> {
                    if("false".equals(supported)&&url.equals(view.getUrl()))
                        failBoot("当前 Android WebView 版本过旧，请更新 Android System WebView 或 Chrome 后重试。");
                });
            }
            @Override public void onReceivedError(WebView view,WebResourceRequest request,WebResourceError error){if(request.isForMainFrame())failBoot("无法连接本机制谱器服务（" + error.getErrorCode() + "）");}
            @Override public void onReceivedHttpError(WebView view,WebResourceRequest request,android.webkit.WebResourceResponse response){if(request.isForMainFrame())failBoot("无法载入制谱器页面（HTTP " + response.getStatusCode() + "）");}
            @Override public boolean shouldOverrideUrlLoading(WebView view,String url){return !url.startsWith(origin);}
            @Override public boolean shouldOverrideUrlLoading(WebView view,WebResourceRequest request){return !request.getUrl().toString().startsWith(origin);}
        });
        web.addJavascriptInterface(new Object(){
            @JavascriptInterface public void ready(){activity.runOnUiThread(()->{
                if(destroyed)return;booting=false;handler.removeCallbacks(timeout);loading.setVisibility(View.GONE);
            });}
            @JavascriptInterface public void bootStage(String text){activity.runOnUiThread(()->{
                if(destroyed||!booting||text==null||text.length()>160)return;
                bootStage=text;loadingText.setText(text);
            });}
            @JavascriptInterface public void bootFailed(String text){activity.runOnUiThread(()->{
                if(text!=null&&text.length()<=160)failBoot(text);
            });}
            @JavascriptInterface public void closeBoot(){activity.runOnUiThread(()->requestBootExit());}
            @JavascriptInterface public boolean directoryPicker(){return true;}
        },"PhiraEditorShell");
        web.setWebChromeClient(new WebChromeClient(){
            @Override public boolean onShowFileChooser(WebView view,ValueCallback<Uri[]> callback,FileChooserParams params){
                return picker.show(callback,params);
            }
        });
        root.addView(web,new FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT,ViewGroup.LayoutParams.MATCH_PARENT));
        loading=new LinearLayout(activity);loading.setOrientation(LinearLayout.VERTICAL);loading.setGravity(Gravity.CENTER);loading.setPadding(24,16,24,16);loading.setBackgroundColor(Color.rgb(23,29,38));
        loadingText=new TextView(activity);loadingText.setTextColor(Color.WHITE);loadingText.setTextSize(16);loadingText.setGravity(Gravity.CENTER);loading.addView(loadingText);
        LinearLayout actions=new LinearLayout(activity);actions.setGravity(Gravity.CENTER);loading.addView(actions);
        Button back=new Button(activity);back.setAllCaps(false);back.setText("返回 Pro");back.setOnClickListener(v->requestBootExit());actions.addView(back);
        retry=new Button(activity);retry.setText("重新加载");retry.setOnClickListener(v->load(false));actions.addView(retry);
        safe=new Button(activity);safe.setText("兼容模式重试");safe.setOnClickListener(v->load(true));actions.addView(safe);
        root.addView(loading,new FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT,ViewGroup.LayoutParams.MATCH_PARENT));
        activity.addContentView(root,new ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT,ViewGroup.LayoutParams.MATCH_PARENT));
        load(false);
    }
    private void load(boolean safeMode){
        web.stopLoading();
        startBoot();
        Uri.Builder url=Uri.parse(address).buildUpon();
        if(safeMode)url.appendQueryParameter("mods","off").appendQueryParameter("skin","default");
        web.loadUrl(url.build().toString());
    }
    private void startBoot(){if(destroyed)return;booting=true;bootStage="正在载入制谱器页面";loading.setVisibility(View.VISIBLE);loadingText.setText(bootStage);retry.setVisibility(View.GONE);safe.setVisibility(View.GONE);handler.removeCallbacks(timeout);handler.postDelayed(timeout,50000);}
    private void failBoot(String message){if(destroyed||!booting)return;handler.removeCallbacks(timeout);loadingText.setText(message);retry.setVisibility(View.VISIBLE);safe.setVisibility(View.VISIBLE);}
    private void requestBootExit(){if(!destroyed&&booting)bootExit.set(true);}
    boolean takeBootExit(){return bootExit.getAndSet(false);}
    void visible(boolean visible){root.setVisibility(visible?View.VISIBLE:View.GONE);if(visible){web.onResume();web.requestFocus();}else{web.evaluateJavascript("globalThis.rpenMods?.editor.pause()",null);web.onPause();}}
    boolean back(){if(root.getVisibility()!=View.VISIBLE)return false;if(booting)requestBootExit();else web.evaluateJavascript("globalThis.phiraEditor?.requestClose()",null);return true;}
    boolean result(int request,int code,Intent data){
        return picker.result(request,code,data);
    }
    void destroy(){destroyed=true;handler.removeCallbacks(timeout);picker.destroy();web.stopLoading();web.removeJavascriptInterface("PhiraEditorShell");web.destroy();if(root.getParent()!=null)((ViewGroup)root.getParent()).removeView(root);}
}
