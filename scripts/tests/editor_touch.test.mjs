import test from 'node:test';
import assert from 'node:assert/strict';
import { TouchController } from '../../assets/rpen-editor/pro/touch.mjs';

function setup(id='notes', tool=1) {
  globalThis.document={addEventListener(){},querySelector(){return null;}};globalThis.window={addEventListener(){}};
  globalThis.phiraEditor={pointerMode:'edit'};
  globalThis.PointerEvent=class {constructor(type,options){Object.assign(this,{type,...options});}};
  let controller;
  globalThis.HTMLCanvasElement=class {
    constructor(){this.id=id;}
    closest(){return true;}
    setPointerCapture(){}
    dispatchEvent(event){event.target=this;controller.handle(event);}
  };
  const canvas=new HTMLCanvasElement();let placements=0,position=4;
  const editor={timeline:{scale:500,tool,down(){placements++;},up(){},drag:null},chartSeconds:()=>position,seek:time=>{position=time;},invalidate(){}};
  controller=new TouchController(editor,()=>{});
  function send(type,id,x,y,pointerType='touch'){const event={type,pointerId:id,pointerType,clientX:x,clientY:y,target:canvas,preventDefault(){this.prevented=true;},stopImmediatePropagation(){this.consumed=true;}};controller.handle(event);return event;}
  return{controller,editor,send,get placements(){return placements;},get position(){return position;}};
}
test('a two-finger gesture cancels placement and retains both pointers until release',()=>{
  const state=setup();state.send('pointerdown',1,10,20);state.send('pointerdown',2,110,20);
  assert.equal(state.controller.points.size,2);
  state.send('pointermove',2,210,20);assert.equal(state.editor.timeline.scale,1000);
  state.send('pointerup',2,210,20);state.send('pointerup',1,10,20);
  assert.equal(state.placements,0);assert.equal(state.controller.points.size,0);
});
test('single-finger placement commits once on release and never after cancellation',()=>{
  const state=setup();state.send('pointerdown',1,10,20);assert.equal(state.placements,0);state.send('pointerup',1,10,20);assert.equal(state.placements,1);
  state.send('pointerdown',2,10,20);state.send('pointercancel',2,10,20);assert.equal(state.placements,1);
});
test('pan changes time without placing objects and clears held input on mode changes',()=>{
  const state=setup();state.controller.setMode('pan');state.send('pointerdown',1,10,20);state.send('pointermove',1,10,120);state.send('pointerup',1,10,120);
  assert.equal(state.placements,0);assert.equal(state.position,4.2);
  state.controller.setMode('box');assert.equal(globalThis.phiraEditor.pointerMode,'box');assert.equal(state.controller.points.size,0);
});
test('touch scrolling captures both note and event drags before selection handlers',()=>{
  for(const id of ['notes','events']) {
    const state=setup(id,0);state.controller.setMode('pan');
    assert.equal(state.send('pointerdown',1,20,50).consumed,true);
    assert.equal(state.send('pointermove',1,20,150).consumed,true);
    assert.equal(state.send('pointerup',1,20,150).consumed,true);
    assert.equal(state.position,4.2);assert.equal(state.placements,0);
    state.controller.setMode('edit');
    assert.equal(state.send('pointerdown',2,20,50).consumed,undefined);
    state.send('pointercancel',2,20,50);
  }
});
test('touch scrolling leaves mouse input and preview canvases unchanged',()=>{
  const mouse=setup();mouse.controller.setMode('pan');
  assert.equal(mouse.send('pointerdown',1,20,50,'mouse').consumed,undefined);
  assert.equal(mouse.controller.points.size,0);
  const preview=setup('realtime-preview');preview.controller.setMode('pan');
  assert.equal(preview.send('pointerdown',1,20,50).consumed,undefined);
  assert.equal(preview.controller.points.size,0);
});
