(()=>{
const {Button,Icon,Modal,TextField}=window.CipheraDesignSystem;
const KEY='ciphera.workspaces';
const GR=['linear-gradient(135deg,#3ec6c0,#3b82f6)','linear-gradient(135deg,#f472b6,#8b5cf6)','linear-gradient(135deg,#fbbf24,#f97316)','linear-gradient(135deg,#34d399,#059669)'];
const INIT={cur:1,list:[{id:1,name:'Ciphera',logo:'',plan:'Pro',members:8},{id:2,name:'Agence Nord',logo:'',plan:'Free',members:3}]};
const THEMES=[{id:'default',name:'Par défaut',c:['#7c4dff','#38bdf8','#f97316','#ec4899','#22c08a']},{id:'ocean',name:'Océan',c:['#2563eb','#06b6d4','#0ea5e9','#6366f1','#14b8a6']},{id:'aurore',name:'Aurore',c:['#a855f7','#ec4899','#f472b6','#8b5cf6','#f0abfc']},{id:'braise',name:'Braise',c:['#ef4444','#f97316','#f59e0b','#dc2626','#fb923c']},{id:'foret',name:'Forêt',c:['#16a34a','#22c55e','#84cc16','#10b981','#65a30d']},{id:'mono',name:'Sobre',c:['#71717a','#a1a1aa','#52525b','#3f3f46','#d4d4d8']}];
const A=[.28,.16,.22,.14,.10];
const hexA=(h,a)=>{const n=parseInt(h.slice(1),16);return 'rgba('+(n>>16&255)+','+(n>>8&255)+','+(n&255)+','+a+')'};
const themeColors=t=>!t||t.id==='default'?THEMES[0].c:t.id==='custom'?[t.a,t.b,t.a,t.b,t.a]:(THEMES.find(x=>x.id===t.id)||THEMES[0]).c;
window.cipheraThemeBg=t=>{const c=themeColors(t);const p=[['38% 60% at 8% 0%'],['30% 55% at 52% -10%'],['34% 60% at 96% 4%'],['40% 50% at 70% 110%'],['35% 50% at 0% 100%']];return p.map((q,i)=>'radial-gradient('+q[0]+','+hexA(c[i],A[i])+',transparent 70%)').join(',')+',#050505'};
const tsubs=new Set();let preview=null;
const curTheme=()=>{if(preview)return preview;const st=load();const w=st.list.find(x=>x.id===st.cur)||st.list[0];return w&&w.theme};
const emit=()=>tsubs.forEach(f=>f({}));
window.previewCipheraTheme=t=>{preview=t;emit()};
window.useCipheraTheme=()=>{const [,f]=React.useState();React.useEffect(()=>{tsubs.add(f);return()=>tsubs.delete(f)},[]);return curTheme()};
const load=()=>{try{return JSON.parse(localStorage.getItem(KEY))||INIT}catch(e){return INIT}};
window.useCipheraWorkspace=()=>{const [,f]=React.useState();React.useEffect(()=>{tsubs.add(f);return()=>tsubs.delete(f)},[]);const st=load();const ws=st.list.find(x=>x.id===st.cur)||st.list[0];return {ws,colors:themeColors(preview||ws.theme),grad:GR[ws.g??(ws.id-1)%GR.length]}};
function Logo({w,size=18}){return w.logo?<img src={w.logo} alt="" style={{width:size,height:size,borderRadius:size*.3,objectFit:'cover',flex:'none',background:'#fff'}}/>:<span style={{width:size,height:size,borderRadius:size*.3,flex:'none',background:GR[w.g??(w.id-1)%GR.length],display:'flex',alignItems:'center',justifyContent:'center',font:`600 ${size*.45}px/1 var(--font-sans)`,color:'#fff'}}>{size>20?(w.name||'?')[0].toUpperCase():''}</span>}
function WsModal({ws,onClose,onSave}){
  const [name,setName]=React.useState(ws?.name||'');const [logo,setLogo]=React.useState(ws?.logo||'');const [mode,setMode]=React.useState('upload');const [url,setUrl]=React.useState(ws?.logo&&!ws.logo.startsWith('data:')?ws.logo:'');const [drag,setDrag]=React.useState(false);const [urlErr,setUrlErr]=React.useState(false);const fileRef=React.useRef();
  const [theme,setTheme]=React.useState(ws?.theme||{id:'default'});const [ca,setCa]=React.useState(ws?.theme?.a||'#7c4dff');const [cb,setCb]=React.useState(ws?.theme?.b||'#f97316');
  const pickTheme=t=>{setTheme(t);window.previewCipheraTheme(t)};
  React.useEffect(()=>()=>window.previewCipheraTheme(null),[]);
  const [gi,setGi]=React.useState(ws?.g??((ws?.id||1)-1)%GR.length);
  const read=f=>{if(!f||!f.type.startsWith('image/'))return;const r=new FileReader();r.onload=()=>setLogo(r.result);r.readAsDataURL(f)};
  const slug=(name||'nouvel-espace').toLowerCase().normalize('NFD').replace(/[\u0300-\u036f]/g,'').replace(/[^a-z0-9]+/g,'-').replace(/^-|-$/g,'');
  const applyUrl=()=>{const u=url.trim();const img=new Image();img.onload=()=>{setLogo(u);setUrlErr(false)};img.onerror=()=>setUrlErr(true);img.src=u};
  React.useEffect(()=>{const f=e=>e.key==='Escape'&&onClose();addEventListener('keydown',f);return()=>removeEventListener('keydown',f)},[]);
  const seg=(k,l,ic)=><button key={k} onClick={()=>setMode(k)} style={{flex:1,display:'flex',alignItems:'center',justifyContent:'center',gap:7,height:32,border:0,borderRadius:8,cursor:'pointer',background:mode===k?'rgba(255,255,255,.08)':'transparent',boxShadow:mode===k?'inset 0 0 0 1px rgba(255,255,255,.08)':'none',color:mode===k?'var(--text-strong)':'var(--text-muted)',font:'500 12px/1 var(--font-sans)',transition:'all .15s'}}><Icon name={ic} size={13}/>{l}</button>;
  const L=t=><span style={{font:'500 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>{t}</span>;
  const inp={width:'100%',height:40,padding:'0 12px',borderRadius:10,border:'1px solid var(--border-strong)',background:'rgba(255,255,255,.02)',color:'var(--text-strong)',font:'400 14px/1 var(--font-sans)',outline:'none',boxSizing:'border-box'};
  return ReactDOM.createPortal(<div onMouseDown={e=>e.target===e.currentTarget&&onClose()} style={{position:'fixed',inset:0,zIndex:1100,display:'flex',justifyContent:'center',overflowY:'auto',padding:'24px 16px',boxSizing:'border-box',background:'rgba(0,0,0,.35)',animation:'cphPop .18s ease-out'}}>
    <div style={{margin:'auto',flex:'none',width:520,maxWidth:'100%',borderRadius:20,overflow:'hidden',background:'#0f0f11',border:'1px solid rgba(255,255,255,.08)',boxShadow:'0 30px 80px -20px rgba(0,0,0,.8)'}}>
      <div style={{position:'relative',height:112,background:logo?'#141416':GR[gi],overflow:'hidden'}}>
        {theme.id!=='default'&&!logo&&<div style={{position:'absolute',inset:0,background:window.cipheraThemeBg(theme).replace(/,#050505$/,''),opacity:.9}}></div>}
        {logo&&<img src={logo} alt="" style={{position:'absolute',inset:-40,width:'calc(100% + 80px)',height:'calc(100% + 80px)',objectFit:'cover',filter:'blur(40px) saturate(1.4)',opacity:.55}}/>}
        <div style={{position:'absolute',inset:0,backgroundImage:'radial-gradient(rgba(255,255,255,.12) 1px,transparent 1px)',backgroundSize:'12px 12px',maskImage:'linear-gradient(180deg,#000,transparent)',WebkitMaskImage:'linear-gradient(180deg,#000,transparent)'}}></div>
        <div style={{position:'absolute',inset:0,background:'linear-gradient(180deg,transparent 30%,#0f0f11)'}}></div>
        <button onClick={onClose} title="Fermer" style={{position:'absolute',top:14,right:14,width:30,height:30,borderRadius:99,border:0,cursor:'pointer',background:'rgba(0,0,0,.35)',color:'#fff',display:'flex',alignItems:'center',justifyContent:'center'}}><Icon name="x" size={15}/></button>
        <span style={{position:'absolute',top:18,left:22,font:'500 11px/1 var(--font-sans)',letterSpacing:'.08em',textTransform:'uppercase',color:'rgba(255,255,255,.75)'}}>{ws?'Modifier l\'espace':'Nouvel espace'}</span>
      </div>
      <div style={{padding:'0 22px',marginTop:-40,position:'relative',display:'flex',alignItems:'flex-end',gap:16}}>
        <div onClick={()=>{setMode('upload');fileRef.current.click()}} title="Changer le logo" style={{position:'relative',width:80,height:80,borderRadius:22,padding:4,background:'#0f0f11',cursor:'pointer',flex:'none'}}>
          {logo?<img src={logo} alt="" style={{width:'100%',height:'100%',borderRadius:18,objectFit:'cover',background:'#fff'}}/>:<span style={{width:'100%',height:'100%',borderRadius:18,background:GR[gi],display:'flex',alignItems:'center',justifyContent:'center',font:'600 30px/1 var(--font-sans)',color:'#fff'}}>{(name||'?')[0].toUpperCase()}</span>}
          <span style={{position:'absolute',right:-2,bottom:-2,width:28,height:28,borderRadius:99,background:'#ededed',color:'#0b0b0b',display:'flex',alignItems:'center',justifyContent:'center',boxShadow:'0 0 0 3px #0f0f11'}}><Icon name="camera" size={13}/></span>
        </div>
        <div style={{display:'flex',flexDirection:'column',gap:7,paddingBottom:8,minWidth:0}}>
          <span style={{font:'600 18px/1.1 var(--font-sans)',color:'var(--text-strong)',overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'}}>{name||'Nouvel espace'}</span>
          <span style={{font:'400 12px/1 var(--font-mono)',color:'var(--text-subtle)'}}>ciphera.dev/{slug}</span>
        </div>
      </div>
      <div style={{padding:'22px 22px 20px',display:'flex',flexDirection:'column',gap:20}}>
        <div style={{display:'flex',flexDirection:'column',gap:8}}>{L('Nom de l\'espace')}<input autoFocus value={name} onChange={e=>setName(e.target.value)} placeholder="Acme SAS" style={inp} onFocus={e=>e.target.style.borderColor='var(--gray-700)'} onBlur={e=>e.target.style.borderColor='var(--border-strong)'}/></div>
        <div style={{display:'flex',flexDirection:'column',gap:10}}>
          <div style={{display:'flex',alignItems:'center',justifyContent:'space-between'}}>{L('Avatar de l\'espace')}{logo&&<button onClick={()=>{setLogo('');setUrl('')}} style={{display:'flex',alignItems:'center',gap:5,background:'none',border:0,padding:0,cursor:'pointer',color:'var(--text-subtle)',font:'500 12px/1 var(--font-sans)'}}><Icon name="trash-2" size={12}/>Retirer</button>}</div>
          <div style={{display:'flex',gap:2,padding:3,borderRadius:11,background:'rgba(255,255,255,.03)',border:'1px solid var(--border-default)'}}>{seg('upload','Importer','upload')}{seg('url','Depuis un lien','link')}</div>
          {mode==='upload'?<div onClick={()=>fileRef.current.click()} onDragOver={e=>{e.preventDefault();setDrag(true)}} onDragLeave={()=>setDrag(false)} onDrop={e=>{e.preventDefault();setDrag(false);read(e.dataTransfer.files[0])}} style={{display:'flex',alignItems:'center',gap:14,padding:'14px 16px',borderRadius:12,border:'1px dashed '+(drag?'#ededed':'rgba(255,255,255,.14)'),background:drag?'rgba(255,255,255,.05)':'rgba(255,255,255,.015)',cursor:'pointer',transition:'all .15s'}}>
            <span style={{width:40,height:40,borderRadius:10,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'rgba(255,255,255,.06)'}}><Icon name="image-up" size={18} color="var(--text-body)"/></span>
            <span style={{display:'flex',flexDirection:'column',gap:6,flex:1}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{drag?'Déposez l\'image':'Glissez une image ou parcourez'}</span><span style={{font:'400 11px/1 var(--font-sans)',color:'var(--text-subtle)'}}>PNG, JPG ou SVG · 256 × 256 px conseillé</span></span>
            <span style={{height:30,padding:'0 12px',borderRadius:8,display:'flex',alignItems:'center',border:'1px solid var(--border-strong)',font:'500 12px/1 var(--font-sans)',color:'var(--text-body)'}}>Parcourir</span>
          </div>:<div style={{display:'flex',flexDirection:'column',gap:6}}><div style={{display:'flex',gap:8}}><div style={{position:'relative',flex:1}}><span style={{position:'absolute',left:12,top:13,display:'flex'}}><Icon name="link" size={14} color="var(--text-subtle)"/></span><input value={url} onChange={e=>{setUrl(e.target.value);setUrlErr(false)}} onKeyDown={e=>e.key==='Enter'&&applyUrl()} placeholder="https://exemple.com/logo.png" style={{...inp,paddingLeft:34,fontFamily:'var(--font-mono)',fontSize:12,borderColor:urlErr?'rgba(239,68,68,.6)':'var(--border-strong)'}}/></div><Button disabled={!/^https?:\/\/.+/.test(url)} onClick={applyUrl}>Appliquer</Button></div>{urlErr&&<span style={{font:'400 11px/1 var(--font-sans)',color:'#f87171'}}>Impossible de charger cette image.</span>}</div>}
          <input ref={fileRef} type="file" accept="image/*" style={{display:'none'}} onChange={e=>read(e.target.files[0])}/>
          {!logo&&<div style={{display:'flex',alignItems:'center',gap:10,marginTop:2}}><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'}}>Ou une couleur</span>{GR.map((g,i)=><button key={i} onClick={()=>setGi(i)} style={{width:22,height:22,borderRadius:99,border:0,padding:0,cursor:'pointer',background:g,boxShadow:gi===i?'0 0 0 2px #0f0f11, 0 0 0 4px #ededed':'none',transition:'box-shadow .15s'}}></button>)}</div>}
        </div>
      </div>
      <div style={{padding:'0 22px 22px',display:'flex',flexDirection:'column',gap:10}}>
        <div style={{display:'flex',alignItems:'center',justifyContent:'space-between'}}>{L('Couleurs du dashboard')}{theme.id!=='default'&&<button onClick={()=>pickTheme({id:'default'})} style={{display:'flex',alignItems:'center',gap:5,background:'none',border:0,padding:0,cursor:'pointer',color:'var(--text-subtle)',font:'500 12px/1 var(--font-sans)'}}><Icon name="rotate-ccw" size={12}/>Par défaut</button>}</div>
        <div style={{display:'grid',gridTemplateColumns:'repeat(4,minmax(0,1fr))',gap:8}}>
          {[...THEMES,{id:'custom',name:'Personnalisé'}].map(t=>{const on=theme.id===t.id;const c=t.id==='custom'?[ca,cb,ca,cb,ca]:t.c;return <button key={t.id} onClick={()=>pickTheme(t.id==='custom'?{id:'custom',a:ca,b:cb}:{id:t.id})} style={{display:'flex',flexDirection:'column',gap:7,padding:5,borderRadius:11,cursor:'pointer',border:'1px solid '+(on?'#ededed':'rgba(255,255,255,.08)'),background:on?'rgba(255,255,255,.05)':'transparent',transition:'border-color .15s'}}>
            <span style={{height:34,borderRadius:7,display:'block',background:'radial-gradient(70% 120% at 0% 0%,'+hexA(c[0],.85)+',transparent 70%),radial-gradient(60% 120% at 100% 0%,'+hexA(c[2],.8)+',transparent 70%),radial-gradient(60% 100% at 50% 0%,'+hexA(c[1],.6)+',transparent 70%),#0b0b0d',position:'relative'}}>{t.id==='custom'&&<Icon name="pipette" size={13} color="#fff" style={{position:'absolute',right:6,bottom:6}}/>}</span>
            <span style={{display:'flex',alignItems:'center',gap:5,padding:'0 3px 2px',font:'500 11px/1 var(--font-sans)',color:on?'var(--text-strong)':'var(--text-muted)'}}>{on&&<Icon name="check" size={11}/>}{t.name}</span>
          </button>})}
        </div>
        {theme.id==='custom'&&<div style={{display:'flex',gap:8}}>{[['Couleur gauche',ca,v=>{setCa(v);pickTheme({id:'custom',a:v,b:cb})}],['Couleur droite',cb,v=>{setCb(v);pickTheme({id:'custom',a:ca,b:v})}]].map(([l,v,f])=><label key={l} style={{flex:1,display:'flex',alignItems:'center',gap:10,height:40,padding:'0 6px 0 12px',borderRadius:10,border:'1px solid var(--border-strong)',cursor:'pointer'}}><span style={{flex:1,font:'400 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>{l}</span><span style={{font:'400 12px/1 var(--font-mono)',color:'var(--text-body)'}}>{v}</span><span style={{position:'relative',width:28,height:28,borderRadius:7,background:v,overflow:'hidden',boxShadow:'inset 0 0 0 1px rgba(255,255,255,.15)'}}><input type="color" value={v} onChange={e=>f(e.target.value)} style={{position:'absolute',inset:0,opacity:0,cursor:'pointer'}}/></span></label>)}</div>}
        <span style={{font:'400 11px/1 var(--font-sans)',color:'var(--text-faint)'}}>Aperçu en direct sur le dashboard derrière cette fenêtre.</span>
      </div>
      <div style={{display:'flex',alignItems:'center',gap:8,padding:'14px 22px',borderTop:'1px solid rgba(255,255,255,.06)',background:'rgba(255,255,255,.015)'}}>
        <span style={{flex:1,display:'flex',alignItems:'center',gap:6,font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'}}><Icon name="users" size={13}/>Visible par tous les membres</span>
        <Button onClick={onClose}>Annuler</Button><Button variant="inverse" disabled={!name.trim()} onClick={()=>onSave({name:name.trim(),logo,g:gi,theme})}>{ws?'Enregistrer':'Créer l\'espace'}</Button>
      </div>
    </div>
  </div>,document.body);
}
function WorkspaceSwitcher(){
  const [st,setSt]=React.useState(load);const [open,setOpen]=React.useState(false);const [modal,setModal]=React.useState(null);const ref=React.useRef();const menuRef=React.useRef();const [pos,setPos]=React.useState({x:0,y:0});
  React.useEffect(()=>{try{localStorage.setItem(KEY,JSON.stringify(st))}catch(e){}emit()},[st]);
  React.useEffect(()=>{if(!open)return;const f=e=>{if(!ref.current.contains(e.target)&&!(menuRef.current&&menuRef.current.contains(e.target)))setOpen(false)};document.addEventListener('mousedown',f);return()=>document.removeEventListener('mousedown',f)},[open]);
  const cur=st.list.find(w=>w.id===st.cur)||st.list[0];
  const save=d=>{if(modal==='new'){const id=Math.max(...st.list.map(w=>w.id))+1;setSt(s=>({cur:id,list:[...s.list,{id,plan:'Free',members:1,...d}]}))}else setSt(s=>({...s,list:s.list.map(w=>w.id===modal.id?{...w,...d}:w)}));setModal(null)};
  return <div ref={ref} style={{position:'relative'}}>
    <button onClick={()=>{const b=ref.current.getBoundingClientRect();setPos({x:b.left,y:b.bottom+6});setOpen(o=>!o)}} style={{display:'flex',alignItems:'center',gap:8,height:30,padding:'0 10px 0 6px',borderRadius:99,background:'rgba(10,10,14,.45)',backdropFilter:'blur(10px)',WebkitBackdropFilter:'blur(10px)',border:'1px solid rgba(255,255,255,.18)',cursor:'pointer',color:'#fff',font:'500 12px/1 var(--font-sans)'}}><Logo w={cur}/>{cur.name}<Icon name="chevrons-up-down" size={12} color="var(--text-subtle)"/></button>
    {open&&ReactDOM.createPortal(<div ref={menuRef} style={{position:'fixed',top:pos.y,left:pos.x,zIndex:1000,width:270,padding:6,borderRadius:12,background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)',border:'1px solid rgba(255,255,255,.06)'}}>
      <div style={{padding:'6px 8px 8px',font:'400 11px/1 var(--font-sans)',color:'var(--text-subtle)'}}>Espaces</div>
      {st.list.map(w=>{const on=w.id===cur.id;return <div key={w.id} onClick={()=>{setSt(s=>({...s,cur:w.id}));setOpen(false)}} onMouseEnter={e=>e.currentTarget.style.background='var(--surface-selected)'} onMouseLeave={e=>e.currentTarget.style.background='transparent'} style={{display:'flex',alignItems:'center',gap:10,padding:'8px',borderRadius:8,cursor:'pointer'}}>
        <Logo w={w} size={28}/>
        <span style={{display:'flex',flexDirection:'column',gap:5,flex:1,minWidth:0}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)',overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'}}>{w.name}</span><span style={{font:'400 11px/1 var(--font-sans)',color:'var(--text-subtle)'}}>{w.plan} · {w.members} membre{w.members>1?'s':''}</span></span>
        <button title="Modifier" onClick={e=>{e.stopPropagation();setOpen(false);setModal(w)}} style={{width:26,height:26,display:'flex',alignItems:'center',justifyContent:'center',border:0,borderRadius:6,background:'transparent',cursor:'pointer',color:'var(--text-muted)'}}><Icon name="pencil" size={13}/></button>
        {on&&<Icon name="check" size={14} color="var(--text-strong)"/>}
      </div>})}
      <div style={{height:1,background:'var(--border-strong)',margin:'6px 0'}}></div>
      <button onClick={()=>{setOpen(false);setModal('new')}} onMouseEnter={e=>e.currentTarget.style.background='var(--surface-selected)'} onMouseLeave={e=>e.currentTarget.style.background='transparent'} style={{display:'flex',alignItems:'center',gap:10,width:'100%',padding:8,border:0,borderRadius:8,background:'transparent',cursor:'pointer',color:'var(--text-body)',font:'500 13px/1 var(--font-sans)'}}><span style={{width:28,height:28,borderRadius:8,border:'1px dashed var(--gray-500)',display:'flex',alignItems:'center',justifyContent:'center'}}><Icon name="plus" size={14}/></span>Créer un espace entreprise</button>
    </div>,document.body)}
    {modal&&<WsModal ws={modal==='new'?null:modal} onClose={()=>setModal(null)} onSave={save}/>}
  </div>;
}
window.WorkspaceSwitcher=WorkspaceSwitcher;
})();
