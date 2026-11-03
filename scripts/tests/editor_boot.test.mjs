import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import { readFile } from 'node:fs/promises';
import { toggleTopPanel } from '../../assets/rpen-editor/pro/panels.mjs';

const bridge = await readFile(new URL('../../assets/rpen-editor/pro/bridge.js',import.meta.url),'utf8');
const boot = await readFile(new URL('../../assets/rpen-editor/pro/boot.js',import.meta.url),'utf8');
function setup(fetch=async()=>({ok:true,json:async()=>({ok:true})})) {
  const listeners=new Map(),timers=new Map(),controls=new Map();let serial=0,nativeExit=0,nativeReady=0,booting=true,replaced;
  for(const id of ['pro-loading-message','pro-loading-back','pro-loading-retry','pro-loading-safe','pro-loading'])controls.set(id,{hidden:id.includes('retry')||id.includes('safe'),remove(){controls.delete(id);}});
  const context={URL,AbortController,TextEncoder,Uint8Array,Promise,fetch,
    setTimeout(fn){const id=++serial;timers.set(id,fn);return id;},clearTimeout:id=>timers.delete(id),
    addEventListener:(name,fn)=>listeners.set(name,fn),window:{addEventListener(){}},
    document:{currentScript:{src:'http://127.0.0.1:1234/session/pro/bridge.js'},documentElement:{classList:{contains:()=>booting,remove:()=>{booting=false;}}},querySelector:selector=>controls.get(selector.slice(1)),getElementById:id=>controls.get(id)},
    location:{href:'http://127.0.0.1:1234/session/index.html',replace:url=>{replaced=String(url);},reload(){}},
    PhiraEditorShell:{closeBoot(){nativeExit++;},ready(){nativeReady++;}},
  };
  vm.createContext(context);vm.runInContext(bridge,context);vm.runInContext(boot,context);
  return {context,controls,listeners,timers,get nativeExit(){return nativeExit;},get nativeReady(){return nativeReady;},get booting(){return booting;},get replaced(){return replaced;}};
}
test('return during startup reaches native escape without waiting for a hung fetch or editor module',()=>{
  const state=setup(()=>new Promise(()=>{}));state.listeners.get('DOMContentLoaded')();
  state.controls.get('pro-loading-back').onclick();assert.equal(state.nativeExit,1);
});
test('a script error before DOM readiness still exposes retry and safe startup',()=>{
  const state=setup();state.listeners.get('error')();state.listeners.get('DOMContentLoaded')();
  assert.equal(state.controls.get('pro-loading-retry').hidden,false);assert.equal(state.controls.get('pro-loading-safe').hidden,false);
  state.controls.get('pro-loading-safe').onclick();assert.equal(new URL(state.replaced).searchParams.get('mods'),'off');
});
test('initialization deadline rejects instead of leaving an endless pending startup',async()=>{
  const state=setup();const task=state.context.phiraEditor.boot.wait(new Promise(()=>{}),'Preparing');
  [...state.timers.values()].at(-1)();await assert.rejects(task,/初始化超时/);
});
test('startup ready removes both loading surfaces and ignores late error notices',()=>{
  const state=setup();state.context.phiraEditor.boot.ready();state.listeners.get('error')();
  assert.equal(state.booting,false);assert.equal(state.controls.has('pro-loading'),false);assert.equal(state.nativeReady,1);assert.equal(state.timers.size,0);
});
test('stalled host requests abort and clear their deadline',async()=>{
  const state=setup((_,{signal})=>new Promise((_,reject)=>signal.addEventListener('abort',()=>reject(Error('aborted')))));
  const pending=state.context.phiraEditor.request('close','{}');[...state.timers.values()].at(-1)();
  await assert.rejects(pending,/aborted/);assert.equal(state.timers.size,1);
});
test('top panel switching opens only the requested panel and closing restores the dock',()=>{
  let active=null;
  for(const id of ['transport','tools','tracks']){active=toggleTopPanel(active,id);assert.equal(active,id);}
  assert.equal(toggleTopPanel(active,'tracks'),null);
});
