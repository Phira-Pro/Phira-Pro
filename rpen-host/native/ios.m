// GPL-3.0-only. UIKit owns only the editor overlay, never the game renderer.
#import <UIKit/UIKit.h>
#import <WebKit/WebKit.h>

@interface ProRpenView : NSObject <WKNavigationDelegate, WKUIDelegate>
@property(nonatomic,strong) WKWebView *web;
@property(nonatomic,strong) NSString *origin;
@end
@implementation ProRpenView
- (void)webView:(WKWebView *)webView decidePolicyForNavigationAction:(WKNavigationAction *)action decisionHandler:(void (^)(WKNavigationActionPolicy))handler {
    NSURL *url=action.request.URL;
    BOOL local=[url.scheme isEqualToString:@"http"] && [url.host isEqualToString:@"127.0.0.1"] && [url.port isEqual:self.web.URL.port];
    // Initial navigation is checked against the exact address supplied by Rust.
    if (!self.web.URL) local=[action.request.URL.absoluteString hasPrefix:self.origin];
    handler(local ? WKNavigationActionPolicyAllow : WKNavigationActionPolicyCancel);
}
- (UIViewController *)presenter {
    UIViewController *view=self.web.window.rootViewController;
    while(view.presentedViewController) view=view.presentedViewController;
    return view;
}
- (void)webView:(WKWebView *)web runJavaScriptAlertPanelWithMessage:(NSString *)message initiatedByFrame:(WKFrameInfo *)frame completionHandler:(void (^)(void))completion {
    UIAlertController *alert=[UIAlertController alertControllerWithTitle:@"RPEN" message:message preferredStyle:UIAlertControllerStyleAlert];
    [alert addAction:[UIAlertAction actionWithTitle:@"确定" style:UIAlertActionStyleDefault handler:^(UIAlertAction *action){completion();}]];
    [[self presenter] presentViewController:alert animated:YES completion:nil];
}
- (void)webView:(WKWebView *)web runJavaScriptConfirmPanelWithMessage:(NSString *)message initiatedByFrame:(WKFrameInfo *)frame completionHandler:(void (^)(BOOL))completion {
    UIAlertController *alert=[UIAlertController alertControllerWithTitle:@"RPEN" message:message preferredStyle:UIAlertControllerStyleAlert];
    [alert addAction:[UIAlertAction actionWithTitle:@"取消" style:UIAlertActionStyleCancel handler:^(UIAlertAction *action){completion(NO);}]];
    [alert addAction:[UIAlertAction actionWithTitle:@"确定" style:UIAlertActionStyleDefault handler:^(UIAlertAction *action){completion(YES);}]];
    [[self presenter] presentViewController:alert animated:YES completion:nil];
}
@end
static ProRpenView *editor;
void rpen_ios_open(const char *url) {
    NSString *address=[NSString stringWithUTF8String:url];
    dispatch_async(dispatch_get_main_queue(), ^{
        if(editor.web){editor.web.hidden=NO;return;}
        UIWindow *window=nil;
        for(UIWindow *item in UIApplication.sharedApplication.windows) if(item.isKeyWindow){window=item;break;}
        if(!window)window=UIApplication.sharedApplication.windows.firstObject;
        UIView *root=window.rootViewController.view;
        if(!root)return;
        editor=[ProRpenView new];
        NSURL *target=[NSURL URLWithString:address];editor.origin=[NSString stringWithFormat:@"http://127.0.0.1:%@/",target.port];
        WKWebViewConfiguration *configuration=[WKWebViewConfiguration new];
        configuration.websiteDataStore=WKWebsiteDataStore.defaultDataStore;
        configuration.allowsInlineMediaPlayback=YES;
        configuration.mediaTypesRequiringUserActionForPlayback=WKAudiovisualMediaTypeNone;
        editor.web=[[WKWebView alloc] initWithFrame:root.bounds configuration:configuration];
        editor.web.navigationDelegate=editor;editor.web.UIDelegate=editor;
        editor.web.autoresizingMask=UIViewAutoresizingFlexibleWidth|UIViewAutoresizingFlexibleHeight;
        editor.web.opaque=YES;editor.web.backgroundColor=[UIColor colorWithRed:0.09 green:0.11 blue:0.15 alpha:1];
        editor.web.scrollView.bounces=NO;
        if(@available(iOS 11.0,*)) editor.web.scrollView.contentInsetAdjustmentBehavior=UIScrollViewContentInsetAdjustmentNever;
        [root addSubview:editor.web];
        [editor.web loadRequest:[NSURLRequest requestWithURL:target]];
    });
}
void rpen_ios_visible(BOOL visible){dispatch_async(dispatch_get_main_queue(), ^{
    editor.web.hidden=!visible;
    if(!visible)[editor.web evaluateJavaScript:@"globalThis.rpenMods?.editor.pause()" completionHandler:nil];
    if(@available(iOS 15.0,*))[editor.web setAllMediaPlaybackSuspended:!visible completionHandler:nil];
});}
void rpen_ios_close(void){dispatch_async(dispatch_get_main_queue(), ^{
    [editor.web stopLoading];[editor.web removeFromSuperview];editor.web.navigationDelegate=nil;editor.web.UIDelegate=nil;editor=nil;
});}
