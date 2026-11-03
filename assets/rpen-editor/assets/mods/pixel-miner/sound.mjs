export function createSounds(editor,volume){
  const sources=new Set();let noise=null;
  function play(kind){
    const loudness=Math.max(0,volume());if(!loudness)return;
    try{const context=editor.audio.ensureContext();if(context.state==='suspended')context.resume();const now=context.currentTime;const gain=context.createGain();const duration=kind==='break'?0.18:kind==='chip'?0.055:0.13;gain.gain.setValueAtTime(loudness*(kind==='chip'?0.14:0.25),now);gain.gain.exponentialRampToValueAtTime(0.0001,now+duration);gain.connect(context.destination);let source,filter;
      if(['chip','break','place'].includes(kind)){if(!noise||noise.sampleRate!==context.sampleRate){noise=context.createBuffer(1,context.sampleRate*0.2,context.sampleRate);const values=noise.getChannelData(0);for(let i=0;i<values.length;i++)values[i]=(Math.random()*2-1)*(0.6+0.4*Math.sin(i*0.7));}source=context.createBufferSource();source.buffer=noise;source.playbackRate.value=kind==='break'?0.8:1.3;filter=context.createBiquadFilter();filter.type='lowpass';filter.frequency.value=kind==='chip'?1800:2600;source.connect(filter);filter.connect(gain);}
      else{source=context.createOscillator();source.type='triangle';source.frequency.setValueAtTime(kind==='xp'?650+Math.random()*170:980,now);source.frequency.exponentialRampToValueAtTime(kind==='xp'?1200:1500,now+duration);source.connect(gain);}
      sources.add(source);source.onended=()=>{sources.delete(source);source.disconnect();filter?.disconnect();gain.disconnect();};source.start(now);source.stop(now+duration);
    }catch{ /* Audio may be unavailable until the next user gesture. */ }
  }
  return{play,dispose(){for(const source of sources){try{source.stop();}catch{}source.disconnect();}sources.clear();}};
}
