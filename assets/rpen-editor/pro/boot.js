// GPL-3.0-only. Keep unfinished home, skin and extension layouts behind one screen.
(() => {
  let failure;
  const booting = () => document.documentElement.classList.contains('pro-booting');
  const stage = text => {const message=document.querySelector('#pro-loading-message');if(message)message.textContent=text;};
  const fail = () => {
    if (!booting()) return;
    failure=true;stage('RPEN 加载未完成，可以重试、停用扩展重试或返回 Pro');
    for(const id of ['pro-loading-retry','pro-loading-safe']){const button=document.getElementById(id);if(button)button.hidden=false;}
  };
  const timeout = setTimeout(fail, 20000);
  globalThis.phiraEditor.boot = {
    stage, fail,
    async wait(task, label, milliseconds=30000) {
      stage(label);let timer;
      try {return await Promise.race([task,new Promise((_,reject)=>{timer=setTimeout(()=>reject(Error('制谱器初始化超时')),milliseconds);})]);}
      finally {clearTimeout(timer);}
    },
    ready() {
      clearTimeout(timeout);
      document.documentElement.classList.remove('pro-booting');
      document.querySelector('#pro-loading')?.remove();
      globalThis.PhiraEditorShell?.ready();
    },
  };
  addEventListener('error', fail);
  addEventListener('unhandledrejection', fail);
  addEventListener('DOMContentLoaded', () => {
    document.querySelector('#pro-loading-back').onclick = () => Promise.resolve(globalThis.phiraEditor.requestClose()).catch(fail);
    document.querySelector('#pro-loading-retry').onclick = () => location.reload();
    document.querySelector('#pro-loading-safe').onclick = () => {const url=new URL(location.href);url.searchParams.set('mods','off');location.replace(url);};
    if(failure)fail();
  });
})();
