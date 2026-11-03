// GPL-3.0-only. Adapt input at the editor boundary, preserving mod event hooks.
export function pointerWithMode(event, mode) {
  return new Proxy(event, { get(target, key) {
    if (key === 'shiftKey') return target.shiftKey || mode === 'box';
    if (key === 'ctrlKey') return target.ctrlKey || mode === 'multi';
    const value = Reflect.get(target, key, target);
    return typeof value === 'function' ? value.bind(target) : value;
  } });
}
export function pinchScale(initialScale, initialDistance, distance) {
  return Math.round(Math.max(20, Math.min(2000, initialScale * distance / Math.max(1, initialDistance))));
}
export class TouchController {
  constructor(editor, changed) {
    this.editor = editor; this.mode = 'edit'; this.points = new Map(); this.changed = changed;
    editor.phiraTouch = this;
    for (const type of ['pointerdown', 'pointermove', 'pointerup', 'pointercancel']) document.addEventListener(type, event => this.handle(event), { capture: true, passive: false });
    window.addEventListener('blur', () => this.cancel());
  }
  setMode(mode) { this.cancel(); this.mode = mode; globalThis.phiraEditor.pointerMode=mode; this.changed(); }
  cancel() {
    const points=[...this.points.values()];this.points.clear();this.gesture=null;
    this.cancelTargets(points);this.editor.timeline.drag=null;
  }
  cancelTargets(points){this.cancelling=true;try{for(const point of points)point.target.dispatchEvent(new PointerEvent('pointercancel',{pointerId:point.id,pointerType:'touch',bubbles:false}));}finally{this.cancelling=false;}}
  handle(event) {
    if (this.cancelling || event.pointerType !== 'touch') return;
    const isCanvas = event.target instanceof HTMLCanvasElement && ['notes','events'].includes(event.target.id) && event.target.closest('.stage');
    if (event.type === 'pointerdown') {
      if (!isCanvas) return;
      const point = { id: event.pointerId, x: event.clientX, y: event.clientY, startX: event.clientX, startY: event.clientY, target: event.target };
      this.points.set(event.pointerId, point);
      if (this.points.size === 2) {
        this.cancelTargets([...this.points.values()]);
        const [a, b] = this.points.values();
        this.gesture = { scale: this.editor.timeline.scale, distance: Math.hypot(a.x-b.x,a.y-b.y), y:(a.y+b.y)/2, time:this.editor.chartSeconds() };
      }
      if (this.points.size >= 2 || this.mode === 'pan' || (event.target.id === 'notes' && this.mode === 'edit' && this.editor.timeline.tool)) {
        point.deferred = true;
        event.target.setPointerCapture(event.pointerId);
        this.consume(event);
      }
      return;
    }
    const point = this.points.get(event.pointerId);
    if (!point) return;
    const dy = event.clientY - point.y;
    point.x = event.clientX; point.y = event.clientY;
    if (event.type === 'pointermove') {
      if (this.gesture && this.points.size >= 2) {
        const [a, b] = this.points.values(), middle=(a.y+b.y)/2;
        this.editor.timeline.scale = pinchScale(this.gesture.scale,this.gesture.distance,Math.hypot(a.x-b.x,a.y-b.y));
        for(const id of ['#y-scale','#y-scale-slider']) { const input=document.querySelector(id);if(input)input.value=this.editor.timeline.scale; }
        this.editor.seek(Math.max(0,this.gesture.time-(middle-this.gesture.y)/this.editor.timeline.scale));
        this.editor.invalidate(); this.consume(event);
      } else if (this.mode === 'pan') { this.editor.seek(Math.max(0,this.editor.chartSeconds()+dy/this.editor.timeline.scale));this.consume(event); }
      else if (point.deferred || this.gesture) this.consume(event);
      return;
    }
    if (event.type === 'pointerup' || event.type === 'pointercancel') {
      const gesture = this.gesture;
      this.points.delete(event.pointerId);
      if (event.type === 'pointerup' && point.deferred && !gesture && this.mode === 'edit' && Math.hypot(point.x-point.startX,point.y-point.startY)<14 && point.target.id==='notes') {
        this.editor.timeline.down(pointerWithMode(event,this.mode));
        this.editor.timeline.up(pointerWithMode(event,this.mode));
      } else if (this.mode === 'box' && this.editor.timeline.drag?.kind === 'rectangle') this.editor.timeline.drag.finished = true;
      if (point.deferred || gesture || this.mode === 'pan') this.consume(event);
      if (!this.points.size) this.gesture = null;
    }
  }
  consume(event) { event.preventDefault(); event.stopImmediatePropagation(); }
}
