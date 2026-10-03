"""Package Android native output and a portable iOS source tree, without signing."""
import hashlib,json,subprocess,zipfile
from pathlib import Path

root=Path(__file__).resolve().parents[2]
parent=root.parent
ios=parent/'iOS'
changed=subprocess.check_output(['git','diff','--name-only'],cwd=root,text=True).splitlines()
changed += ['phira/src/chart_install.rs','phira/src/leaderboard.rs','prpr/src/scene/game/inner.rs','prpr/src/core/block_audio.rs','prpr/src/core/block_touch.rs','prpr/src/core/block_shader_full.frag','prpr/src/core/block_shader_full.vert','assets/blockarea/FD_Noise_00000.png','docs/pro-fixes-v4.md','docs/block-area/README.md']
deleted={path for path in changed if not (root/path).is_file()}
overlays={path:root/path for path in changed if (root/path).is_file() and (path.startswith(('phira/src/','prpr/src/','assets/','docs/')) or path in ('prpr/Cargo.toml','phira.app/Info.plist'))}
for directory in ('sasa','prpr-miniquad'):
    for p in (parent/'vendor'/directory/'src').rglob('*.rs'):
        overlays[p.relative_to(parent).as_posix()]=p
roots=('assets','phira','prpr','phira-main','phira-monitor','phira-mp','phira-mp-next','prpr-auto-offset','prpr-avc','prpr-l10n','prpr-pbc','phira.xcodeproj','scripts','tools','vendor','xcode','.cargo','.github')
base={p.relative_to(ios).as_posix():p for d in roots for p in (ios/d).rglob('*') if p.is_file()}
for filename in ('Cargo.toml','Cargo.lock','LICENSE','README.md','README-zh_CN.md','rust-toolchain.toml','rustfmt.toml','.gitignore','phira.app/Info.plist'):
    p=ios/filename
    if p.is_file():base[filename]=p
for path in deleted:base.pop(path,None)
base.update(overlays)
base={n:p for n,p in base.items() if not set(Path(n).parts)&{'target','.git','cache','__pycache__','node_modules','xcuserdata'} and not n.endswith(('.keystore','.p12','.mobileprovision','LocalSigning.xcconfig'))}
assert base['Cargo.toml']==ios/'Cargo.toml'
assert 'path = "vendor/prpr-miniquad"' in base['Cargo.toml'].read_text('utf8')
assert base['phira/src/chart_install.rs']==root/'phira/src/chart_install.rs'
assert base['prpr/src/scene/game/inner.rs']==root/'prpr/src/scene/game/inner.rs'

out=parent/'PhiraPro-iOS-sources-v4.zip'
with zipfile.ZipFile(out,'w',zipfile.ZIP_DEFLATED,compresslevel=6) as z:
    for name,p in sorted(base.items()):z.write(p,name)
    z.writestr('VERIFICATION-v4.json',json.dumps({'platform':'iOS sources','application_version':'0.8.2-pro.6','activation_required':False,'compiled_on_mac':False,'device_verified':False,'overlaid_sha256':{n:hashlib.sha256(p.read_bytes()).hexdigest() for n,p in overlays.items()}},indent=2))
    z.writestr('README-v4.txt','iOS 源码包，不能直接安装。保留原 iOS 工程的本地 vendor/Cargo/Xcode 配置，叠加第四轮修复；尚未在 Mac/Xcode 编译或高刷实机验证。请按已有构建脚本在 Mac 编译。成绩只走 Pro，空配置关闭上传。颜色及部分 Ready 帧尚有差异，详见 docs/block-area/README.md。未打包签名密钥或用户谱面。')
with zipfile.ZipFile(out) as z:
    assert z.testzip() is None
    for n,p in overlays.items():assert hashlib.sha256(z.read(n)).digest()==hashlib.sha256(p.read_bytes()).digest()
sha=hashlib.sha256(out.read_bytes()).hexdigest();out.with_suffix('.zip.sha256').write_text(f'{sha} *{out.name}\n','utf8')
print(json.dumps({'package':str(out),'bytes':out.stat().st_size,'sha256':sha,'files':len(base)},ensure_ascii=False))

lib=root/'target/aarch64-linux-android/release/libphira.so'
native_sources=[p for directory in ('phira/src','prpr/src') for p in (root/directory).rglob('*') if p.suffix in ('.rs','.frag','.vert')]
native_sources += [root/'Cargo.toml',root/'Cargo.lock',root/'prpr/Cargo.toml']
native_sources += [p for directory in ('sasa','prpr-miniquad') for p in (parent/'vendor'/directory/'src').rglob('*.rs')]
assert lib.stat().st_mtime >= max(p.stat().st_mtime for p in native_sources), 'Android library is older than source; rebuild before packaging'
b=lib.read_bytes();assert b[:4]==b'\x7fELF' and int.from_bytes(b[18:20],'little')==183
out=parent/'Android/PhiraPro-native-arm64-v4.zip'
with zipfile.ZipFile(out,'w',zipfile.ZIP_DEFLATED,compresslevel=6) as z:
    z.write(lib,'lib/arm64-v8a/libphira.so')
    z.write(root/'target/review-v4-android-build.log','verification/build.log')
    z.write(root/'docs/pro-fixes-v4.md','docs/pro-fixes-v4.md')
    z.write(root/'docs/block-area/README.md','docs/block-area/README.md')
    z.writestr('verification/manifest.json',json.dumps({'target':'aarch64-linux-android','ndk_api':26,'application_version':'0.8.2-pro.6','activation_required':False,'configuration':'Release, --cfg record','library_sha256':hashlib.sha256(b).hexdigest(),'signed_apk':False,'device_verified':False},indent=2))
    z.writestr('README.txt','Android arm64 原生库构建产物，不是可安装 APK。NDK API=26、Release、--cfg record。沿用现有前端/资源生成 APK 后，必须使用原发布密钥签名；没有尝试读取或猜测签名密码。成绩只走 Pro。未做小米平板高刷实机测试。')
with zipfile.ZipFile(out) as z:assert z.testzip() is None and z.read('lib/arm64-v8a/libphira.so')==b
sha=hashlib.sha256(out.read_bytes()).hexdigest();out.with_suffix('.zip.sha256').write_text(f'{sha} *{out.name}\n','utf8')
print(json.dumps({'package':str(out),'bytes':out.stat().st_size,'sha256':sha},ensure_ascii=False))
