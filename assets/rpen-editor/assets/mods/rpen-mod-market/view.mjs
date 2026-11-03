import {marketText,marketName} from './text.mjs';
const node=(tag,text='',className='')=>{const el=document.createElement(tag);el.textContent=text;el.className=className;return el;};
export function nativeMarketView(api,transport){
  const dialog=node('dialog','','rpen-native-market');dialog.dataset.modInteractive='true';api.mount(dialog);
  const outerStyle=node('style');outerStyle.textContent='.rpen-native-market{width:min(1130px,96vw);height:90vh;max-width:none;max-height:none;padding:0;border:1px solid #766b47;background:#262723;color:#eee;overflow:auto}.rpen-native-market::backdrop{background:#10110fe0}';api.mount(outerStyle);
  const shell=node('div');dialog.append(shell);const shadow=shell.attachShadow({mode:'open'});
  const style=node('style');style.textContent=`:host{--gold:#ffdc70;--line:#4c4b43;--muted:#aaa69b;font:14px RPE,'Microsoft YaHei',sans-serif;color:#eee}*{box-sizing:border-box}button,input,select,textarea{font:inherit;border:1px solid var(--line);background:#33342f;color:inherit;padding:9px 14px;border-radius:2px}button{cursor:pointer}button:hover{border-color:var(--gold);color:var(--gold)}button:disabled{opacity:.4}header{display:flex;justify-content:space-between;align-items:center;padding:17px 25px;border-bottom:1px solid var(--line);position:sticky;top:0;background:#262723;z-index:2}header strong{color:var(--gold);font-size:18px;font-weight:400}header button{border:0;background:none;padding:0 7px;font-size:23px}main{padding:28px}h1{font:900 76px/.9 Arial,sans-serif;letter-spacing:-5px;margin:10px 0 30px}h1 small{font:15px RPE,sans-serif;letter-spacing:6px;color:var(--gold);display:block;margin-top:18px}.tools{display:flex;gap:10px;margin-bottom:24px;flex-wrap:wrap}.tools input{flex:1;min-width:140px}.active{color:var(--gold);border-color:#a89251;background:#4d4632}.grid{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:25px}.tile{cursor:pointer;min-width:0}.tile:hover{transform:translateY(-3px)}.art{height:140px;background:linear-gradient(150deg,#414538,#2b2d29);display:grid;place-items:center;overflow:hidden;color:#ffdc7050;font:900 60px Arial}.art img{width:100%;height:100%;object-fit:cover}.tile h2{font-size:17px;font-weight:400;margin:14px 0 8px}.tile p{font-size:12px;line-height:1.8;color:var(--muted);display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;overflow:hidden}.release{display:grid;grid-template-columns:1fr 1.2fr;gap:32px;align-items:start}.release .art{height:240px}.release h2{font-size:34px;font-weight:400;margin:0 0 18px}.meta{color:var(--muted);font-size:12px;margin:18px 0}.actions{display:flex;gap:9px;flex-wrap:wrap}.primary{background:var(--gold);color:#282921;border-color:var(--gold)}.intro{white-space:pre-wrap;line-height:1.9;margin:30px 0;color:#d2cfc2;overflow-wrap:anywhere}h3{font-size:18px;font-weight:400;padding:20px 0 12px;border-bottom:1px solid var(--line)}details{border-bottom:1px solid var(--line);padding:16px 0}summary{color:var(--gold);cursor:pointer}details p{white-space:pre-wrap;color:var(--muted);line-height:1.8;font-size:13px}.empty{height:200px;display:grid;place-items:center;font-size:70px;color:#ffdc7020}textarea{width:100%;min-height:180px;resize:vertical}.feedback{max-width:700px;margin:0 auto}.feedback h2{font-size:27px;font-weight:400}.update-scope{border-top:1px solid var(--line);padding:16px 0 24px;margin-bottom:20px}.scope-copy{color:var(--muted);font-size:12px;line-height:1.8}.update-scope>select{margin:12px 12px 12px 0}.choices{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:8px;margin-bottom:16px}.choice{display:flex;align-items:center;gap:9px;font-size:13px}.choice input{accent-color:var(--gold)}.retry{display:block;margin:15vh auto;font-size:36px;border:0;background:none;color:var(--gold)}.pager{display:flex;gap:15px;justify-content:center;margin:30px}.shake{animation:shake .3s}@keyframes shake{25%,75%{transform:translateX(-4px)}50%{transform:translateX(4px)}}@media(max-width:700px){main{padding:20px}.grid{grid-template-columns:repeat(2,minmax(0,1fr));gap:18px}.release{grid-template-columns:1fr}.art{height:120px}.release h2{font-size:28px}h1{font-size:60px}}`;
  const header=node('header'),close=node('button','×'),notice=node('section','','notice'),body=node('main');header.append(node('strong',marketText('title')),close);shadow.append(style,header,notice,body);close.onclick=()=>dialog.close();
  style.textContent+='.notice:empty{display:none}.notice{margin:20px 28px 0;padding:15px 18px;border-left:3px solid var(--gold);background:#38362b}.notice p{margin:0 0 12px}.notice button{margin-right:9px}';
  let context,categories=[],tab='all',query='',category='',page=1,controller=null,timer=null,unlock=null,disposed=false,updates=[];
  const urls=new Set();
  function cancelRender(){controller?.abort();clearTimeout(timer);for(const url of urls)URL.revokeObjectURL(url);urls.clear();}
  function nextRender(){cancelRender();controller=new AbortController();return controller.signal;}
  function alive(signal){if(disposed||api.signal.aborted||signal.aborted)throw new DOMException('Market stopped','AbortError');}
  async function showRecovery(){
    if(!await context.recovery())return false;
    notice.replaceChildren(node('p',marketText('recoverySaved')),button(marketText('restorePackages'),async()=>{await context.recover();notice.replaceChildren(node('p',marketText('recoveryRestored')));}),button(marketText('exportPackages'),()=>context.exportBackup()));return true;
  }
  async function problem(error,task){
    console.error(error);if(disposed||api.signal.aborted)return;
    const reason=error?.message?': '+error.message:'';notice.replaceChildren(node('p',marketText(error.rolledBack?'rolledBack':'failed')+reason),button(marketText('retry'),task));
    try{await showRecovery();}catch(storageError){console.error(storageError);}
  }
  async function run(task,target){if(disposed||api.signal.aborted)return;try{await task();}catch(error){if(error.name==='AbortError')return;await problem(error,task);target.classList.remove('shake');void target.offsetWidth;target.classList.add('shake');}}
  const button=(text,task,className='')=>{const b=node('button',text,className);b.type='button';b.onclick=()=>run(task,b);return b;};
  api.listen(shadow,'keydown',event=>event.stopPropagation());api.listen(shadow,'keyup',event=>event.stopPropagation());
  api.listen(dialog,'close',()=>{cancelRender();unlock?.();unlock=null;});
  async function artwork(mod,signal){
    const art=node('div',marketName(mod.name).slice(0,2),'art');
    if(mod.cover){const bytes=await transport.binary('cover',{release:mod.release},{signal});alive(signal);const img=node('img'),url=URL.createObjectURL(new Blob([bytes]));urls.add(url);img.src=url;art.replaceChildren(img);}
    return art;
  }
  async function gridInto(container,result,signal){
    alive(signal);const grid=node('div','','grid');container.replaceChildren(grid);
    for(const mod of result.items){const card=node('div','','tile');card.append(await artwork(mod,signal),node('h2',marketName(mod.name)),node('p',marketName(mod.summary)));alive(signal);card.onclick=()=>run(()=>detail(mod.id),card);grid.append(card);}
    if(!result.items.length)container.append(node('div','◇','empty'));
    if(result.pages>1){const pager=node('div','','pager'),prev=button('←',async()=>{page--;await browse();}),next=button('→',async()=>{page++;await browse();});prev.disabled=page<=1;next.disabled=page>=result.pages;pager.append(prev,node('span',page+' / '+result.pages),next);container.append(pager);}
  }
  async function browse(){
    nextRender();body.replaceChildren();const title=node('h1','MODS');title.append(node('small',marketText('openMarket')));body.append(title);
    const tools=node('div','','tools'),search=node('input'),select=node('select');search.placeholder=marketText('search');search.value=query;
    select.append(new Option(marketText('allCategories'),''));for(const c of categories)select.append(new Option(marketName(c.name),c.id));select.value=category;
    const container=node('div');
    const filtered=()=>({items:updates.filter(m=>(!category||m.category===category)&&(!query||(marketName(m.name)+marketName(m.summary)).includes(query))),pages:1});
    async function load(){const signal=nextRender();const result=tab==='updates'?filtered():await context.api('list',undefined,{q:query,category,page},{signal});alive(signal);await gridInto(container,result,signal);}
    search.oninput=()=>{query=search.value;page=1;clearTimeout(timer);timer=setTimeout(()=>run(load,search),250);};
    select.onchange=()=>{category=select.value;page=1;run(load,select);};
    tools.append(button(marketText('allMods'),async()=>{tab='all';page=1;await browse();},tab==='all'?'active':''),button(marketText('updates'),async()=>{tab='updates';page=1;await browse();},tab==='updates'?'active':''),search,select);body.append(tools);
    if(tab==='updates'){
      const section=node('section','','update-scope'),copy=node('p',marketText('updateDisclosure',{endpoint:context.endpoint()}),'scope-copy'),scope=node('select'),choices=node('div','','choices');
      scope.append(new Option(marketText('marketOnly'),'market'),new Option(marketText('manualSelection'),'selected'));scope.value=context.scope();
      function showChoices(){choices.replaceChildren();if(scope.value!=='selected')return;const selected=context.selected();for(const item of context.candidates()){const label=node('label','','choice'),input=node('input');input.type='checkbox';input.checked=selected.has(item.id);input.onchange=()=>{const ids=context.selected();input.checked?ids.add(item.id):ids.delete(item.id);context.setScope('selected',[...ids]);updates=[];run(load,input);};label.append(input,node('span',marketName(item.name)));choices.append(label);}}
      scope.onchange=()=>{context.setScope(scope.value);updates=[];showChoices();run(load,scope);};showChoices();
      const check=button(marketText('checkUpdates'),async()=>{check.disabled=true;const signal=nextRender();try{updates=await context.check({signal});alive(signal);await gridInto(container,filtered(),signal);}finally{check.disabled=false;}},'primary');
      section.append(copy,scope,choices,check);body.append(section);
    }
    body.append(container);await load();
  }
  async function installRelease(mod,button){
    button.disabled=true;
    try{await context.install(mod.id,mod.release);if(!disposed&&!api.signal.aborted){notice.replaceChildren();updates=updates.filter(item=>item.id!==mod.id);await detail(mod.id);}}
    finally{button.disabled=false;}
  }
  async function detail(id,release){
    const signal=nextRender(),mod=await context.api('detail',undefined,{id,...(release?{release}:{})},{signal});alive(signal);body.replaceChildren(button(marketText('back'),browse));
    const head=node('div','','release'),info=node('div');info.append(node('h2',marketName(mod.name)),node('p',marketName(mod.summary)),node('p',marketName(mod.author||mod.manifest.author)+' · v'+mod.version,'meta'));
    const actions=node('div','','actions'),install=button(marketText(context.has(id)?'reinstall':'install'),()=>installRelease(mod,install),'primary');actions.append(install,button(marketText('feedback'),()=>feedback(mod)),button(marketText('copyRelease'),()=>navigator.clipboard.writeText(mod.url)));info.append(actions);head.append(await artwork(mod,signal),info);alive(signal);body.append(head,node('div',marketName(mod.description),'intro'),node('h3',marketText('history')));
    for(const r of mod.history){const item=node('details'),install=button(marketText('installVersion'),()=>installRelease({...mod,release:r.release},install));item.append(node('summary','v'+r.version),node('p',marketName(r.changelog)),install);body.append(item);}
    for(const dependency of mod.manifest.dependencies)body.append(button(dependency,()=>detail(dependency)));
  }
  async function feedback(mod){
    const signal=nextRender();body.replaceChildren(button('← '+marketName(mod.name),()=>detail(mod.id)));const form=node('div','','feedback'),text=node('textarea');text.placeholder=marketText('feedbackPlaceholder');text.maxLength=4000;
    const send=button(marketText('send'),async()=>{send.disabled=true;try{await context.api('feedback',{id:mod.id,release:mod.release,body:text.value},{},{signal});alive(signal);await detail(mod.id);}finally{send.disabled=false;}},'primary');form.append(node('h2',marketText('feedback')),text,send);body.append(form);
  }
  const open=async value=>{
    if(disposed||api.signal.aborted)throw new DOMException('Market stopped','AbortError');context=value;tab=value.updatesOnly?'updates':'all';updates=[];
    if(!dialog.open){unlock=api.lockEditing({reason:marketText('openMarket')});dialog.showModal();}
    const signal=nextRender();
    notice.replaceChildren();
    try{await showRecovery();alive(signal);categories=await value.api('categories',undefined,{},{signal});alive(signal);if(value.id)await detail(value.id);else await browse();}
    catch(error){if(error.name!=='AbortError'){body.replaceChildren(button('↻',()=>open(value),'retry'));await problem(error,()=>open(value));}throw error;}
  };
  open.isOpen=()=>!disposed&&dialog.open;
  open.reset=()=>{cancelRender();dialog.close();updates=[];notice.replaceChildren();};
  open.dispose=()=>{if(disposed)return;disposed=true;cancelRender();unlock?.();unlock=null;dialog.close();dialog.remove();outerStyle.remove();};
  return open;
}
