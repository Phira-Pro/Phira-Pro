import test from 'node:test';
import assert from 'node:assert/strict';
import { ModRuntime, ModScope } from '../../assets/rpen-editor/src/ui/mod-runtime.mjs';
import { disableModalOwner } from '../../assets/rpen-editor/pro/recovery.mjs';

test('modal recovery releases only the owner and preserves editor document and other extensions',async()=>{
  const chart={notes:[1,2,3]}, editor={chart,dirty:true};
  const runtime=new ModRuntime({editor,notify(){},invalidate(){},changed(){}});
  const parent={children:new Set(),append(node){this.children.add(node);}};
  const mount=(id)=>{
    const node={contains:child=>child===node.child,child:{},remove(){parent.children.delete(node);}};
    const state={id,record:{manifest:{id,name:id,settings:{}},files:new Map()},scope:new ModScope(),settings:{},status:'loaded'};
    runtime.loaded.set(id,state);runtime.createApi(state).mount(node,parent);return node;
  };
  const overlay=mount('overlay'),other=mount('other');
  assert.equal(runtime.ownerOf(overlay.child),'overlay');
  let refreshed=0;
  const manager={runtime,disable:id=>runtime.unload(id),renderList(){refreshed++;}};
  await disableModalOwner(manager,runtime.ownerOf(overlay));
  assert.equal(runtime.ownerOf(overlay.child),null);
  assert.equal(runtime.ownerOf(other),'other');
  assert.equal(parent.children.has(other),true);
  assert.equal(parent.children.has(overlay),false);
  assert.equal(editor.chart,chart);assert.equal(editor.dirty,true);assert.equal(refreshed,1);
  await disableModalOwner(manager,'overlay');assert.equal(refreshed,1);
});
