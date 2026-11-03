import {marketText,marketName,runMarketTask} from './text.mjs';
const encode=new TextEncoder();
const hex=bytes=>[...new Uint8Array(bytes)].map(b=>b.toString(16).padStart(2,'0')).join('');
const hash=async bytes=>hex(await crypto.subtle.digest('SHA-256',bytes));
export async function marketContentHash(files){
  const entries=[...files].sort(([a],[b])=>{const x=encode.encode(a),y=encode.encode(b);for(let i=0;i<Math.min(x.length,y.length);i++)if(x[i]!==y[i])return x[i]-y[i];return x.length-y.length;});
  const parts=await Promise.all(entries.map(async([name,bytes])=>name+'\0'+await hash(bytes)+'\n'));
  return hash(encode.encode(parts.join('')));
}
const cancelled=()=>new DOMException(marketText('stopped'),'AbortError');
const checkpoint=signal=>{if(signal.aborted)throw cancelled();};
export function marketRecoveryStore(){
  async function access(mode,action){
    const database=await new Promise((resolve,reject)=>{const request=indexedDB.open('rpen-market-recovery',1);request.onupgradeneeded=()=>request.result.createObjectStore('backup');request.onsuccess=()=>resolve(request.result);request.onerror=()=>reject(request.error);});
    return new Promise((resolve,reject)=>{const transaction=database.transaction('backup',mode),request=action(transaction.objectStore('backup'));transaction.oncomplete=()=>{database.close();resolve(request.result);};transaction.onabort=transaction.onerror=()=>{database.close();reject(transaction.error||Error(marketText('recoveryUnavailable')));};});
  }
  return {read:()=>access('readonly',store=>store.get('install')),save:backup=>access('readwrite',store=>store.put(backup,'install')),clear:()=>access('readwrite',store=>store.delete('install'))};
}
export async function restoreMarketBackup({manager,parsePackage,storeMod,deleteMod,readMods},backup){
  const snapshots=new Map(backup.snapshots),failures=[];
  const attempt=async task=>{try{return await task();}catch(error){console.error(error);return undefined;}};
  for(const key of snapshots.keys())try{await manager.disable(key);}catch(error){failures.push(error);}
  const read=async()=>new Map((await readMods()).map(stored=>{const record=parsePackage(new Map(stored.files));return [record.manifest.id,record];}));
  let actual=await attempt(read);
  const matches=async(key,record)=>record?actual?.has(key)&&await marketContentHash(actual.get(key).files)===await marketContentHash(record.files):actual&&!actual.has(key);
  for(const key of backup.targets)if(!snapshots.get(key).record)await attempt(()=>deleteMod(key));
  for(const key of backup.targets){const record=snapshots.get(key).record;if(record&&!await matches(key,record))await attempt(()=>storeMod(record));}
  actual=await attempt(read);
  for(const key of backup.targets){
    const record=snapshots.get(key).record,matched=await matches(key,record);
    if(!matched)failures.push(Error('Package recovery pending: '+key));
    if(actual?.has(key))manager.records.set(key,matched?record:actual.get(key));else manager.records.delete(key);
  }
  for(const [key,snapshot]of snapshots){
    if(snapshot.option)manager.options[key]=snapshot.options;else delete manager.options[key];
    manager.desired.delete(key);
    if(snapshot.error===undefined)manager.errors.delete(key);else manager.errors.set(key,snapshot.error);
    if(failures.length)manager.errors.set(key,marketText('recoveryPending'));
  }
  manager.selected=backup.selected;
  try{manager.persist();}catch(error){failures.push(error);}
  return failures;
}
export async function installMarketRelease({manager,parsePackage,readZip,storeMod,deleteMod,readMods,journal,api,download,review,signal,ownerId},id,release){
  checkpoint(signal);await manager.ready;checkpoint(signal);
  if(manager.busy)throw Error(marketText('managerBusy'));manager.busy=true;
  const original=new Map(manager.records),plan=new Map(),snapshots=new Map();let changing=false,backup;
  const selected=manager.selected;
  async function resume(key){
    checkpoint(signal);let reject;const stopped=new Promise((yes,no)=>{reject=no;});
    const descriptor=Object.getOwnPropertyDescriptor(manager,'enable'),originalEnable=manager.enable;
    const enable=async function(id,...args){if(snapshots.has(id))checkpoint(signal);const result=await originalEnable.call(this,id,...args);if(snapshots.has(id))checkpoint(signal);return result;};
    manager.enable=enable;
    const cancel=()=>{for(const id of snapshots.keys()){manager.runtime.unload(id).catch(console.error);manager.desired.delete(id);}reject(cancelled());};
    signal.addEventListener('abort',cancel,{once:true});
    try{await Promise.race([manager.enable(key),stopped]);checkpoint(signal);}
    finally{signal.removeEventListener('abort',cancel);if(manager.enable===enable){if(descriptor)Object.defineProperty(manager,'enable',descriptor);else delete manager.enable;}}
  }
  try{
    if(await journal.read())throw Object.assign(Error(marketText('recoveryPending')),{recoveryPending:true});checkpoint(signal);
    async function prepare(key,revision,chain=new Set()){
      checkpoint(signal);if(chain.has(key))throw Error('Dependency cycle');if(plan.has(key))return;
      if(original.get(key)?.builtin||key===ownerId)throw Error('Reserved mod identity');
      const mod=await api('detail',undefined,{id:key,release:revision});checkpoint(signal);
      if(mod?.manifest?.id!==key)throw Error('Package identity mismatch');
      const bytes=await download(mod);checkpoint(signal);
      if(await hash(bytes)!==mod.sha256)throw Error('Package hash mismatch');checkpoint(signal);
      const record=parsePackage(await readZip(bytes,{maxFiles:2048,maxBytes:128*1024*1024,maxFileBytes:32*1024*1024}));checkpoint(signal);
      if(record.manifest.id!==key||record.manifest.version!==mod.version||await marketContentHash(record.files)!==mod.content_hash)throw Error('Package manifest mismatch');checkpoint(signal);
      const next=new Set([...chain,key]);
      for(const dep of record.manifest.dependencies)if(!original.has(dep))await prepare(dep,undefined,next);
      plan.set(key,{record,mod});
    }
    await prepare(id,release);
    const affected=new Set(plan.keys());let grew=true;
    while(grew){grew=false;for(const [key,r]of original)if(!affected.has(key)&&r.manifest.dependencies.some(dep=>affected.has(dep))){affected.add(key);grew=true;}}
    if(affected.has(ownerId))throw Error('Cannot replace an active market dependency');
    const graph=(key,chain=new Set(),done=new Set())=>{if(chain.has(key))throw Error('Dependency cycle');if(done.has(key))return;const record=plan.get(key)?.record||original.get(key);if(!record)throw Error('Missing dependency');const next=new Set([...chain,key]);for(const dep of record.manifest.dependencies)graph(dep,next,done);done.add(key);};
    for(const key of plan.keys())graph(key);
    for(const key of affected)snapshots.set(key,{record:original.get(key),desired:manager.desired.has(key),loaded:manager.runtime.loaded.has(key),enabled:manager.desired.has(key)||manager.runtime.loaded.has(key),option:Object.hasOwn(manager.options,key),options:structuredClone(manager.options[key]),error:manager.errors.get(key)});
    const enabled=[...snapshots].filter(([,s])=>s.enabled).map(([key,s])=>({id:key,name:s.record.manifest.name,version:plan.get(key)?.record.manifest.version||s.record.manifest.version}));
    let restore=false,index=0;
    for(const {record}of plan.values()){
      const result=await review(record,{last:++index===plan.size,enabled,replacing:original.get(record.manifest.id),signal});checkpoint(signal);
      if(!result.accepted)throw cancelled();if(result.restore)restore=true;
    }
    for(const key of affected)if(manager.records.get(key)!==original.get(key))throw Error('Installed mods changed during review');
    backup={targets:[...plan.keys()],snapshots:[...snapshots],selected};
    await journal.save(backup);changing=true;checkpoint(signal);
    for(const [key,snapshot]of snapshots)if(snapshot.enabled){await manager.disable(key);checkpoint(signal);}
    for(const [key,{record}]of plan){checkpoint(signal);await storeMod(record);checkpoint(signal);manager.records.set(key,record);manager.errors.delete(key);}
    if(restore)for(const entry of enabled){await resume(entry.id);if(!manager.runtime.loaded.has(entry.id))throw Error('Mod activation interrupted');}
    manager.selected=id;manager.persist();checkpoint(signal);await journal.clear();
    return {...plan.get(id).mod,restored:restore,disabled:restore?[]:enabled};
  }catch(error){
    if(changing){
      const failures=await restoreMarketBackup({manager,parsePackage,storeMod,deleteMod,readMods},backup);
      if(!failures.length){
        try{
          if(!signal.aborted)for(const [key,snapshot]of snapshots)if(snapshot.loaded&&!signal.aborted)await resume(key);
          for(const [key,snapshot]of snapshots){if(!signal.aborted&&snapshot.desired)manager.desired.add(key);if(snapshot.error===undefined)manager.errors.delete(key);else manager.errors.set(key,snapshot.error);}
          manager.persist();await journal.clear();
        }catch(problem){failures.push(problem);}
      }
      if(failures.length)throw Object.assign(new AggregateError([error,...failures],marketText('recoverySaved')),{recoveryPending:true});
      error.rolledBack=true;
    }
    throw error;
  }finally{manager.busy=false;manager.renderList();}
}
export function createModMarket({manager,parsePackage,readZip,writeZip,storeMod,deleteMod,readMods,journal=marketRecoveryStore(),marketUrl,token=()=>'',mount=node=>document.body.append(node),listen=(node,type,fn,options)=>{node.addEventListener(type,fn,options);return()=>node.removeEventListener(type,fn,options);},transport=null,view=null,signal:parentSignal,ownerId='rpen-mod-market'}){
  const source=()=>typeof marketUrl==='function'?marketUrl():marketUrl;
  let base=new URL(source());base.pathname=base.pathname.replace(/\/?$/,'/');
  let apiUrl=new URL('api.php',base);const bridge=crypto.randomUUID();
  let dialog=null,frame=null,changes=[],scope='market',selectedIds=new Set();
  const lifetime=new AbortController(),signal=lifetime.signal,listeners=[];
  let sourceKey='',sourceLifetime=new AbortController();
  function refreshSource(){
    checkpoint(signal);const next=new URL(source());next.pathname=next.pathname.replace(/\/?$/,'/');
    const address=transport?.refresh?.()||'',key=next.href+'|'+address;
    if(sourceKey&&sourceKey!==key){sourceLifetime.abort();sourceLifetime=new AbortController();setScope('market');view?.reset?.();dialog?.close();}
    sourceKey=key;base=next;apiUrl=new URL('api.php',base);
  }
  const stop=()=>{lifetime.abort();sourceLifetime.abort();for(const off of listeners.splice(0))off();transport?.close();view?.dispose?.();dialog?.close();dialog?.remove();style.remove();marketButton.remove();updateButton.remove();};
  parentSignal?.addEventListener('abort',stop,{once:true});
  async function api(action,data,query={},options={}){
    refreshSource();const requestSignal=AbortSignal.any([signal,sourceLifetime.signal,...(options.signal?[options.signal]:[])]);checkpoint(requestSignal);
    if(transport)return transport.json(action,data,query,{signal:requestSignal});
    const url=new URL(apiUrl);url.searchParams.set('action',action);for(const[k,v]of Object.entries(query))url.searchParams.set(k,v);
    const headers={};if(data!==undefined)headers['Content-Type']='application/json';if(token())headers.Authorization='Bearer '+token();
    const response=await fetch(url,{method:data===undefined?'GET':'POST',headers,body:data===undefined?undefined:JSON.stringify(data),signal:requestSignal});
    const json=await response.json();if(!json.ok)throw Error(json.error?.code);return json.data;
  }
  function fromMarket(record){const market=record.manifest.market;if(!market||market.id!==record.manifest.id)return false;try{return new URL(market.api).href===apiUrl.href;}catch{return false;}}
  function candidates(){refreshSource();return [...manager.records.values()].filter(r=>!r.builtin&&r.manifest.id!==ownerId).map(r=>({id:r.manifest.id,name:r.manifest.name,market:fromMarket(r)}));}
  function setScope(value,ids=[]){scope=value;selectedIds=new Set(ids);changes=[];updateButton.textContent=marketText('checkUpdates');}
  async function installed(){
    refreshSource();const requestSignal=sourceLifetime.signal,records=[...manager.records.values()].filter(r=>!r.builtin&&r.manifest.id!==ownerId&&(scope==='selected'?selectedIds.has(r.manifest.id):fromMarket(r)));
    const result=await Promise.all(records.map(async r=>({id:r.manifest.id,version:r.manifest.version,content_hash:await marketContentHash(r.files)})));checkpoint(signal);checkpoint(requestSignal);return result;
  }
  async function send(type,extra={},includeInstalled=false){checkpoint(signal);const entries=includeInstalled?await installed():[];checkpoint(signal);frame.contentWindow.postMessage({channel:'rpen-market',bridge,type,installed:entries,...extra},base.origin);}
  async function check(options={}){refreshSource();const requestSignal=AbortSignal.any([signal,sourceLifetime.signal,...(options.signal?[options.signal]:[])]),items=await installed();checkpoint(requestSignal);if(!items.length){changes=[];updateButton.textContent=marketText('checkUpdates');return changes;}changes=(await api('check-updates',{installed:items},{},{signal:requestSignal})).items.filter(m=>m.update);checkpoint(requestSignal);updateButton.textContent=changes.length?marketText('updatesAvailable',{count:changes.length}):marketText('checkUpdates');return changes;}
  async function review(record,{last,enabled,replacing,signal:reviewSignal=signal}){
    checkpoint(reviewSignal);const promise=manager.review(record),resolve=manager.warningResolve;
    const label=document.createElement('label'),restore=document.createElement('input');restore.type='checkbox';label.className='mod-consent';
    if(last&&enabled.length){label.append(restore,marketText('restoreEnabled',{mods:enabled.map(item=>marketName(item.name)+' '+item.version).join(', ')}));manager.warning.querySelector('.modal-actions').before(label);}
    if(replacing){const note=document.createElement('p');note.textContent=marketText('replacing',{name:marketName(replacing.manifest.name),version:replacing.manifest.version});manager.warning.querySelector('.modal-actions').before(note);}
    const cancel=()=>{if(manager.warningResolve===resolve)manager.finishWarning(false);};reviewSignal.addEventListener('abort',cancel,{once:true});
    try{return {accepted:await promise,restore:restore.checked};}finally{reviewSignal.removeEventListener('abort',cancel);}
  }
  async function install(id,release){
    refreshSource();const operationSignal=AbortSignal.any([signal,sourceLifetime.signal]);
    return installMarketRelease({manager,parsePackage,readZip,storeMod,deleteMod,readMods,journal,api:(action,data,query)=>api(action,data,query,{signal:operationSignal}),review,signal:operationSignal,ownerId,download:async mod=>{
      if(transport)return transport.binary('download',{release:mod.release},{signal:operationSignal});
      const response=await fetch(new URL(mod.download,base),{headers:token()?{Authorization:'Bearer '+token()}:{},signal:operationSignal});if(!response.ok)throw Error('Package download rejected');return response.arrayBuffer();
    }},id,release);
  }
  async function recover(){
    checkpoint(signal);await manager.ready;if(manager.busy)throw Error(marketText('managerBusy'));manager.busy=true;
    try{const backup=await journal.read();if(!backup)return;const failures=await restoreMarketBackup({manager,parsePackage,storeMod,deleteMod,readMods},backup);if(failures.length)throw Object.assign(new AggregateError(failures,marketText('recoverySaved')),{recoveryPending:true});await journal.clear();}
    finally{manager.busy=false;manager.renderList();}
  }
  async function exportBackup(){
    const backup=await journal.read();if(!backup)return;
    const files=new Map();for(const [id,snapshot]of backup.snapshots)if(snapshot.record)files.set(id+'.rpenmod',new Uint8Array(await(await writeZip(snapshot.record.files)).arrayBuffer()));
    files.set('recovery.json',encode.encode(JSON.stringify({selected:backup.selected,mods:backup.snapshots.map(([id,s])=>({id,desired:s.desired,options:s.options}))},null,2)));
    const blob=await writeZip(files),url=URL.createObjectURL(blob),link=document.createElement('a');link.href=url;link.download='rpen-market-recovery.zip';link.click();setTimeout(()=>URL.revokeObjectURL(url),30000);
  }
  async function open(id='',updatesOnly=false){
    checkpoint(signal);await manager.ready;checkpoint(signal);
    refreshSource();
    setScope('market');
    if(view){manager.dialog.close();await view({id,updatesOnly,api,install,installed,check,candidates,setScope,scope:()=>scope,selected:()=>new Set(selectedIds),has:id=>manager.records.has(id),endpoint:()=>transport?.endpoint?new URL(transport.endpoint()).origin:apiUrl.origin,recovery:()=>journal.read(),recover,exportBackup});return;}
    if(!dialog){
      dialog=document.createElement('dialog');dialog.className='rpen-market-dialog';dialog.dataset.modInteractive='true';
      const bar=document.createElement('div');bar.className='rpen-market-bar';const close=document.createElement('button');close.textContent='×';close.onclick=()=>dialog.close();bar.append(close);
      frame=document.createElement('iframe');frame.title='模组市场';dialog.append(bar,frame);mount(dialog);
      listeners.push(listen(window,'message',async event=>{
        if(event.source!==frame.contentWindow||event.origin!==base.origin||event.data?.channel!=='rpen-market'||event.data?.bridge!==bridge)return;
        if(event.data.type==='ready')await send('host',{token:token(),recoveryPending:!!await journal.read()});
        if(event.data.type==='recover'||event.data.type==='export-backup'){
          try{if(event.data.type==='recover')await recover();else await exportBackup();await send('recovery-state',{recoveryPending:!!await journal.read(),recovered:event.data.type==='recover'});}
          catch(error){console.error(error);if(!signal.aborted)await send('recovery-state',{recoveryPending:true});}return;
        }
        if(event.data.type==='check-updates'){
          try{await check();await send('updates',{updates:changes},true);}
          catch(error){if(error.name!=='AbortError'){console.error(error);await send('check-failed');}}
        }
        if(event.data.type==='install'){
          try{const mod=await install(event.data.id,event.data.release);await send('installed',{id:mod.id,installed:[{id:mod.id,version:mod.version,content_hash:mod.content_hash}]});}
          catch(error){if(error.name!=='AbortError')console.error(error);if(!signal.aborted&&error.name!=='AbortError')await send('install-failed',{id:event.data.id,recoveryPending:!!await journal.read(),message:marketText(error.rolledBack?'rolledBack':'installFailed')});else if(!signal.aborted)await send('install-cancelled',{id:event.data.id});}
        }
      }));
    }
    const url=new URL(base);url.searchParams.set('bridge',bridge);url.searchParams.set('parent_origin',location.origin);if(id)url.searchParams.set('mod',id);if(updatesOnly)url.searchParams.set('tab','updates');frame.src=url.href;
    manager.dialog.close();dialog.showModal();
  }
  const style=document.createElement('style');style.textContent='.rpen-market-dialog{width:min(1180px,96vw);height:92vh;max-width:none;max-height:none;padding:0;border:1px solid #696450;background:#242424;color:#eee;overflow:hidden}.rpen-market-dialog::backdrop{background:#10110fe0}.rpen-market-bar{position:absolute;right:0;top:0;height:29px;z-index:2;display:flex;justify-content:flex-end}.rpen-market-bar button{height:29px;width:35px;padding:0;border:0;border-radius:0;background:#36362e;color:#ffdc70;font-size:22px}.rpen-market-dialog iframe{width:100%;height:100%;border:0;display:block}.market-publish-link{margin:10px 0;display:inline-block;color:var(--accent,#ffdc70)}';mount(style);
  const marketButton=document.createElement('button');marketButton.textContent=marketText('openMarket');marketButton.id='mods-market';marketButton.onclick=()=>runMarketTask(()=>open()).catch(console.error);
  const updateButton=document.createElement('button');updateButton.textContent=marketText('checkUpdates');updateButton.id='mods-market-updates';updateButton.onclick=()=>runMarketTask(()=>open('',true)).catch(console.error);
  manager.dialog.querySelector('.mod-manager-toolbar').prepend(marketButton,updateButton);
  if(parentSignal?.aborted)stop();
  return {open,check,install,installed,candidates,setScope,refresh:refreshSource,recover,exportBackup,dispose:()=>{parentSignal?.removeEventListener('abort',stop);stop();},buttons:[marketButton,updateButton],style,dialog:()=>dialog};
}
