(()=>{
const DEF=/*EDITMODE-BEGIN*/{"accents":"vives","radius":"arrondis","glows":true,"banners":"couleur","density":"normale"}/*EDITMODE-END*/;
function css(t){
  let c='';
  const R=[[18,{moyens:10,carres:4}],[17,{moyens:10,carres:4}],[16,{moyens:9,carres:4}],[14,{moyens:8,carres:3}],[12,{moyens:7,carres:3}],[11,{moyens:6,carres:3}],[10,{moyens:6,carres:2}],[9,{moyens:5,carres:2}]];
  if(t.radius!=='arrondis')R.forEach(([px,m])=>{c+='#root [style*="border-radius: '+px+'px"]{border-radius:'+m[t.radius]+'px!important}'});
  if(t.radius==='carres')c+='#root [style*="border-radius: 99px"]:not([style*="width: 6px"]):not([style*="width: 7px"]):not([style*="width: 8px"]):not([style*="width: 5px"]){border-radius:4px!important}';
  if(!t.glows)c+='#root span[style*="radial-gradient"]{display:none!important}#root [style*="box-shadow: 0 0 "]{box-shadow:none!important}#root [style*="0 0 8px rgba"],#root [style*="0 0 12px"]{box-shadow:none!important}';
  if(t.banners==='neutres')c+='#root [style*="linear-gradient(110deg"],#root [style*="linear-gradient(100deg"]{background:#141417!important;border-color:rgba(255,255,255,.1)!important}';
  const sat={vives:1,sobres:.55,mono:0}[t.accents];
  if(sat!==1)c+='#root main{filter:saturate('+sat+')}';
  if(t.density==='compacte')c+='#root main{zoom:.92}';
  return c;
}
function DashTweaks(){
  const [t,set]=useTweaks(DEF);
  React.useEffect(()=>{let el=document.getElementById('dash-tweaks');if(!el){el=document.createElement('style');el.id='dash-tweaks';document.head.appendChild(el)}el.textContent=css(t)},[t]);
  return <TweaksPanel>
    <TweakSection label="Style"/>
    <TweakRadio label="Couleurs d'accent" value={t.accents} options={['vives','sobres','mono']} onChange={v=>set('accents',v)}/>
    <TweakRadio label="Bannières" value={t.banners} options={['couleur','neutres']} onChange={v=>set('banners',v)}/>
    <TweakToggle label="Lueurs et dégradés décoratifs" value={t.glows} onChange={v=>set('glows',v)}/>
    <TweakSection label="Forme"/>
    <TweakRadio label="Coins" value={t.radius} options={['arrondis','moyens','carres']} onChange={v=>set('radius',v)}/>
    <TweakRadio label="Densité" value={t.density} options={['normale','compacte']} onChange={v=>set('density',v)}/>
  </TweaksPanel>;
}
window.DashTweaks=DashTweaks;
})();
