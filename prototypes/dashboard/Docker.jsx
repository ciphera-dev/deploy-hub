(()=>{
const {Button,IconButton,Icon,Select,Modal}=window.CipheraDesignSystem;
const lbl={font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'};
const line='1px solid rgba(255,255,255,.06)';
const box={borderRadius:'var(--radius-xl)',border:'1px solid var(--border-default)',background:'var(--surface-app)'};
const C=(o)=>({cpu:0,mem:0,memMax:512,ports:[],up:'—',restarts:0,...o});
const DATA={
  'srv-par-01':[
    {stack:'ciphera-web',items:[C({id:'a1f3',n:'web-1',img:'registry.example.com/web:4821',st:'running',cpu:38,mem:236,memMax:512,ports:['3000→80'],up:'4 h'}),C({id:'b7c2',n:'web-2',img:'registry.example.com/web:4821',st:'running',cpu:31,mem:221,memMax:512,ports:['3000'],up:'4 h'}),C({id:'c9d1',n:'worker-1',img:'registry.example.com/worker:4821',st:'running',cpu:22,mem:388,memMax:1024,up:'4 h'}),C({id:'d4e8',n:'postgres-1',img:'postgres:17-alpine',st:'running',cpu:12,mem:1126,memMax:2048,ports:['5432'],up:'12 j'})]},
    {stack:'billing-api',items:[C({id:'e2a9',n:'api-1',img:'registry.example.com/billing:4807',st:'restarting',cpu:3,mem:96,memMax:512,ports:['8080'],up:'—',restarts:7}),C({id:'f1b0',n:'redis-1',img:'redis:7-alpine',st:'running',cpu:2,mem:42,memMax:256,ports:['6379'],up:'12 j'})]},
    {stack:'Système',items:[C({id:'0aa1',n:'caddy',img:'caddy:2.8',st:'running',cpu:4,mem:58,memMax:256,ports:['80','443'],up:'31 j'}),C({id:'0bb2',n:'ciphera-agent',img:'ciphera/agent:0.9.4',st:'running',cpu:1,mem:34,memMax:128,up:'31 j'}),C({id:'0cc3',n:'crowdsec',img:'crowdsecurity/crowdsec:v1.6',st:'running',cpu:3,mem:112,memMax:256,up:'31 j'}),C({id:'0dd4',n:'old-migration',img:'registry.example.com/web:4790',st:'exited',up:'—'})]},
  ],
  'srv-par-02':[{stack:'analytics',items:[C({id:'1aa1',n:'clickhouse-1',img:'clickhouse/clickhouse-server:24.8',st:'running',cpu:16,mem:2460,memMax:4096,ports:['8123','9000'],up:'6 j'}),C({id:'1bb2',n:'ingest-1',img:'registry.example.com/ingest:212',st:'running',cpu:9,mem:180,memMax:512,up:'6 j'})]}],
  'srv-fra-01':[],
};
const IMAGES=[['registry.example.com/web','4821','412 Mo','il y a 4 h',2],['registry.example.com/worker','4821','398 Mo','il y a 4 h',1],['postgres','17-alpine','238 Mo','il y a 12 j',1],['redis','7-alpine','41 Mo','il y a 12 j',1],['caddy','2.8','49 Mo','il y a 31 j',1],['registry.example.com/web','4790','409 Mo','il y a 5 j',0]];
const VOLUMES=[['ciphera-web_pgdata','39,7 Go','postgres-1'],['ciphera-web_uploads','4,2 Go','web-1, web-2'],['billing-api_redis','118 Mo','redis-1'],['caddy_data','12 Mo','caddy']];
const ST={running:['En marche','#86efac'],restarting:['Redémarre','#fbbf24'],exited:['Arrêté','#71717a'],paused:['En pause','#7dd3fc']};
function Dot({s}){const [,c]=ST[s];return <span style={{position:'relative',width:8,height:8,flex:'none'}}><span style={{position:'absolute',inset:0,borderRadius:99,background:c}}></span>{s==='running'&&<span style={{position:'absolute',inset:-3,borderRadius:99,border:'1px solid '+c,opacity:.35}}></span>}</span>}
function Meter({v,max,unit}){const p=Math.min(100,v/max*100);return <div style={{display:'flex',flexDirection:'column',gap:6,minWidth:0}}><span style={{font:'400 12px/1 var(--font-mono)',color:'var(--text-body)'}}>{unit==='%'?v+' %':(v>=1024?(v/1024).toFixed(1).replace('.',',')+' Go':v+' Mo')}</span><span style={{height:3,borderRadius:99,background:'rgba(255,255,255,.07)',overflow:'hidden'}}><span style={{display:'block',height:'100%',width:p+'%',borderRadius:99,background:p>=85?'#f87171':p>=70?'#fbbf24':'#ededed'}}></span></span></div>}
function Menu({items,onClose}){
  React.useEffect(()=>{const f=()=>onClose();setTimeout(()=>document.addEventListener('click',f),0);return()=>document.removeEventListener('click',f)},[]);
  return <div style={{position:'absolute',top:32,right:0,zIndex:30,width:200,padding:4,background:'var(--surface-tooltip)',borderRadius:12,boxShadow:'var(--shadow-tooltip)'}}>{items.map((it,i)=>it==='-'?<div key={i} style={{height:1,background:'var(--border-strong)',margin:'4px 0'}}></div>:<button key={i} onClick={it[2]} onMouseEnter={e=>e.currentTarget.style.background='var(--surface-selected)'} onMouseLeave={e=>e.currentTarget.style.background='transparent'} style={{display:'flex',alignItems:'center',gap:10,width:'100%',height:32,padding:'0 10px',border:0,borderRadius:8,background:'transparent',cursor:'pointer',textAlign:'left',color:it[3]?'#f87171':'var(--text-body)',font:'400 13px/1 var(--font-sans)'}}><Icon name={it[0]} size={14}/>{it[1]}</button>)}</div>;
}
const COLS='minmax(150px,1.2fr) minmax(120px,1.4fr) 90px 90px minmax(0,110px) 56px 96px';
function CRow({c,first,act}){
  const [h,setH]=React.useState(false);const [menu,setMenu]=React.useState(false);const on=c.st==='running';
  return <div onMouseEnter={()=>setH(true)} onMouseLeave={()=>setH(false)} style={{display:'grid',gridTemplateColumns:COLS,gap:16,alignItems:'center',minHeight:60,padding:'10px 16px',borderTop:first?'none':'1px solid var(--border-subtle)',background:h?'rgba(255,255,255,.02)':'transparent',opacity:c.st==='exited'?.6:1}}>
    <span style={{display:'flex',alignItems:'center',gap:12,minWidth:0}}><Dot s={c.st}/><span style={{display:'flex',flexDirection:'column',gap:5,minWidth:0}}><span style={{font:'500 13px/1 var(--font-mono)',color:'var(--text-strong)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{c.n}</span><span style={{...lbl,fontSize:11,color:c.st==='restarting'?'#fbbf24':'var(--text-subtle)'}}>{ST[c.st][0]}{c.restarts?' · '+c.restarts+' redémarrages':''}</span></span></span>
    <span style={{font:'400 12px/1 var(--font-mono)',color:'var(--text-muted)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{c.img}</span>
    {on||c.st==='restarting'?<Meter v={c.cpu} max={100} unit="%"/>:<span style={lbl}>—</span>}
    {on||c.st==='restarting'?<Meter v={c.mem} max={c.memMax}/>:<span style={lbl}>—</span>}
    <span style={{display:'flex',gap:4,flexWrap:'wrap'}}>{c.ports.length?c.ports.map(p=><span key={p} style={{height:20,display:'flex',alignItems:'center',padding:'0 6px',borderRadius:5,background:'rgba(255,255,255,.05)',font:'400 11px/1 var(--font-mono)',color:'var(--text-body)'}}>{p}</span>):<span style={lbl}>—</span>}</span>
    <span style={{...lbl}}>{c.up}</span>
    <div style={{display:'flex',alignItems:'center',justifyContent:'flex-end',gap:2,opacity:h||menu?1:.55,transition:'opacity .15s'}}>
      <IconButton size={28} variant="ghost" icon="scroll-text" label="Logs" onClick={()=>act('logs',c)}/>
      <IconButton size={28} variant="ghost" icon={on?'square':'play'} label={on?'Arrêter':'Démarrer'} onClick={()=>act(on?'stop':'start',c)}/>
      <div style={{position:'relative'}}><IconButton size={28} variant="ghost" icon="ellipsis" label="Plus" onClick={()=>setMenu(m=>!m)}/>{menu&&<Menu onClose={()=>setMenu(false)} items={[['rotate-ccw','Redémarrer',()=>act('restart',c)],['terminal','Ouvrir un shell',()=>act('shell',c)],['info','Inspecter',()=>act('inspect',c)],'-',['trash-2','Supprimer',()=>act('rm',c),true]]}/>}</div>
    </div>
  </div>;
}
function Empty({onCompose}){
  const [open,setOpen]=React.useState(false);
  React.useEffect(()=>{if(!open)return;const f=()=>setOpen(false);setTimeout(()=>document.addEventListener('click',f),0);return()=>document.removeEventListener('click',f)},[open]);
  return <div style={{...box,display:'flex',flexDirection:'column',alignItems:'center',textAlign:'center',gap:12,padding:'56px 24px 52px'}}>
    <div style={{position:'relative',width:96,height:70,marginBottom:12}}>
      <div style={{position:'absolute',inset:0,borderRadius:10,border:'1px solid rgba(255,255,255,.12)',background:'#141418',padding:'9px 10px',display:'flex',flexDirection:'column',gap:7}}><span style={{display:'flex',gap:3}}>{[0,1,2].map(i=><span key={i} style={{width:5,height:5,borderRadius:99,background:'rgba(255,255,255,.25)'}}></span>)}</span><span style={{height:7,borderRadius:3,background:'rgba(255,255,255,.18)'}}></span><span style={{height:7,width:'75%',borderRadius:3,background:'rgba(255,255,255,.12)'}}></span></div>
      <span style={{position:'absolute',right:-16,bottom:-14,width:36,height:36,borderRadius:99,display:'flex',alignItems:'center',justifyContent:'center',background:'#0c0c0f',border:'1px solid rgba(255,255,255,.14)'}}><Icon name="container" size={16} color="var(--text-body)"/></span>
    </div>
    <span style={{font:'600 16px/1.2 var(--font-sans)',color:'var(--text-strong)'}}>Aucun conteneur sur ce serveur</span>
    <span style={{maxWidth:400,font:'400 13px/1.55 var(--font-sans)',color:'var(--text-muted)'}}>Lancez une image manuellement, importez un docker-compose depuis un dépôt, ou partez d'un modèle prêt à l'emploi.</span>
    <div style={{position:'relative',marginTop:8}}>
      <Button variant="inverse" iconRight="chevron-down" onClick={()=>setOpen(o=>!o)}>Composer</Button>
      {open&&<div style={{position:'absolute',top:40,left:'50%',transform:'translateX(-50%)',zIndex:30,width:240,padding:4,background:'var(--surface-tooltip)',borderRadius:12,boxShadow:'var(--shadow-tooltip)',textAlign:'left'}}>{[['box','Lancer une image'],['file-code','Coller un docker-compose.yml'],['github','Importer depuis un dépôt'],['layout-template','Choisir un modèle']].map(([ic,t])=><button key={t} onClick={()=>onCompose(t)} onMouseEnter={e=>e.currentTarget.style.background='var(--surface-selected)'} onMouseLeave={e=>e.currentTarget.style.background='transparent'} style={{display:'flex',alignItems:'center',gap:10,width:'100%',height:34,padding:'0 10px',border:0,borderRadius:8,background:'transparent',cursor:'pointer',color:'var(--text-body)',font:'400 13px/1 var(--font-sans)'}}><Icon name={ic} size={14}/>{t}</button>)}</div>}
    </div>
  </div>;
}
function LogsModal({c,onClose}){
  const L=[['info','Listening on 0.0.0.0:3000'],['info','GET /health 200 2ms'],['info','GET /api/projects 200 41ms'],['warn','Slow query: SELECT * FROM deployments (812ms)'],['info','POST /api/deploy 202 18ms'],['info','GET /health 200 1ms']];
  return <Modal title={'Logs · '+c.n} description={c.img} onClose={onClose} width={720}>
    <pre className="log-scroll" style={{margin:0,maxHeight:340,overflow:'auto',padding:'14px 16px',borderRadius:12,background:'#08080a',border:'1px solid rgba(255,255,255,.07)',font:'400 12px/1.8 var(--font-mono)',color:'var(--text-body)'}}>{L.map((l,i)=><div key={i}><span style={{color:'var(--text-faint)'}}>12:04:{String(10+i*7).padStart(2,'0')}  </span><span style={{color:l[0]==='warn'?'#fbbf24':'var(--text-body)'}}>{l[1]}</span></div>)}</pre>
  </Modal>;
}
function DockerPage(){const [term,setTerm]=React.useState(false);
  const servers=Object.keys(DATA);const [srv,setSrv]=React.useState(servers[0]);const [data,setData]=React.useState(DATA);const [tab,setTab]=React.useState('Conteneurs');const [q,setQ]=React.useState('');const [logs,setLogs]=React.useState(null);const [faq,setFaq]=React.useState(false);const [fold,setFold]=React.useState({});
  const [toast,setToast]=React.useState('');const flash=m=>{setToast(m);clearTimeout(flash.t);flash.t=setTimeout(()=>setToast(''),2200)};
  const groups=data[srv];const all=groups.flatMap(g=>g.items);
  const setSt=(c,st)=>setData(d=>({...d,[srv]:d[srv].map(g=>({...g,items:g.items.map(x=>x.id===c.id?{...x,st,up:st==='running'?'à l\'instant':'—',restarts:0,cpu:st==='running'?Math.max(2,x.cpu||5):0,mem:st==='running'?Math.max(30,x.mem||60):0}:x)}))}));
  const [cf,setCf]=React.useState(null);
  const act=(k,c,ok)=>{if(k==='logs')return setLogs(c);if(!ok&&(k==='stop'||k==='rm'||k==='restart'))return setCf({k,c});if(k==='stop'){setSt(c,'exited');flash(c.n+' arrêté')}else if(k==='start'){setSt(c,'running');flash(c.n+' démarré')}else if(k==='restart'){setSt(c,'running');flash(c.n+' redémarré')}else if(k==='rm'){setData(d=>({...d,[srv]:d[srv].map(g=>({...g,items:g.items.filter(x=>x.id!==c.id)})).filter(g=>g.items.length)}));flash(c.n+' supprimé')}else flash(k==='shell'?'Shell ouvert sur '+c.n:'docker inspect '+c.id)};
  const SUM=[['En marche',all.filter(c=>c.st==='running').length,'#86efac'],['Arrêtés',all.filter(c=>c.st==='exited').length,'#71717a'],['En erreur',all.filter(c=>c.st==='restarting').length,'#fbbf24'],['Images',IMAGES.length,null],['Volumes',VOLUMES.length,null]];
  const TABS=['Conteneurs','Images','Volumes'];
  return <div style={{padding:'20px 32px 40px 34px',display:'flex',flexDirection:'column',gap:16,maxWidth:1240,minWidth:0}}>
    {(()=>{const run=all.filter(c=>c.st==='running');const cpuT=run.reduce((a,c)=>a+c.cpu,0);const memT=run.reduce((a,c)=>a+c.mem,0);
      const Sp=({v})=>{const dd=Array.from({length:16},(_,i)=>v+Math.sin(i/2)*v*.2+((i*7)%5));const w=112,h=44,m=Math.max(...dd),n=Math.min(...dd);const pts=dd.map((x,i)=>[i/(dd.length-1)*w,h-4-(x-n)/((m-n)||1)*(h-10)]);const pa=pts.map((q2,i)=>(i?'L':'M')+q2[0].toFixed(1)+','+q2[1].toFixed(1)).join(' ');const e=pts[pts.length-1];return <svg width={w} height={h} style={{flex:'none',overflow:'visible'}}><path d={pa+' L'+w+','+h+' L0,'+h+' Z'} fill="rgba(139,124,246,.18)"/><path d={pa} fill="none" stroke="#8b7cf6" strokeWidth="1.5"/><circle cx={e[0]} cy={e[1]} r="3.5" fill="#a78bfa"/></svg>};
      const Tl=({t,v,u,bar,sp})=><div style={{display:'flex',flexDirection:'column',gap:14,padding:'18px 20px',borderRadius:14,border:'1px solid var(--border-default)'}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{t}</span><span style={{display:'flex',alignItems:'flex-end',gap:12}}><span style={{display:'flex',alignItems:'baseline',gap:5,flex:1}}><span style={{font:'600 24px/1 var(--font-sans)',color:'var(--text-strong)'}}>{v}</span><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>{u}</span></span>{sp!=null?<Sp v={sp}/>:<span style={{width:112,height:8,borderRadius:99,background:'var(--surface-selected)',overflow:'hidden',marginBottom:6}}><span style={{display:'block',height:'100%',width:bar+'%',borderRadius:99,background:'#8b7cf6'}}></span></span>}</span></div>;
      const sep=<span style={{width:1,height:14,background:'var(--border-strong)'}}></span>;
      return <div style={{borderRadius:20,border:'1px solid var(--border-default)',background:'var(--surface-app)',padding:'22px 24px',display:'flex',flexDirection:'column',gap:18}}>
        <div style={{display:'flex',alignItems:'center',gap:16}}>
          <span style={{width:52,height:52,borderRadius:13,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'#1d63ed'}}><Icon name="container" size={24} color="#fff"/></span>
          <span style={{display:'flex',flexDirection:'column',gap:8,flex:1,minWidth:0}}><span style={{display:'flex',alignItems:'center',gap:10}}><span style={{font:'600 18px/1 var(--font-sans)',color:'var(--text-strong)'}}>Docker 27.3</span><span style={{height:22,display:'flex',alignItems:'center',padding:'0 8px',borderRadius:6,background:all.length?'rgba(34,197,94,.12)':'var(--surface-selected)',font:'500 12px/1 var(--font-sans)',color:all.length?'var(--green-500)':'var(--text-muted)'}}>{all.length?'En marche':'Aucun conteneur'}</span></span><span style={lbl}>{srv} · moteur containerd 1.7</span></span>
          <Select icon="server" options={servers} value={srv} onChange={setSrv}/>
          <Button icon="terminal" onClick={()=>setTerm(true)}>Terminal</Button>{term&&<window.TerminalModal server={srv} onClose={()=>setTerm(false)}/>}
        </div>
        <div style={{display:'flex',alignItems:'center',gap:18,flexWrap:'wrap',padding:'14px 18px',borderRadius:12,background:'var(--surface-raised)',font:'400 13px/1 var(--font-sans)',color:'var(--text-muted)'}}>
          <span>Conteneurs : <span style={{fontFamily:'var(--font-mono)',fontWeight:600,color:'var(--text-strong)'}}>{run.length} / {all.length}</span></span>{sep}
          <span>Images : <span style={{fontFamily:'var(--font-mono)',fontWeight:600,color:'var(--text-strong)'}}>{IMAGES.length}</span></span>{sep}
          <span>Volumes : <span style={{fontFamily:'var(--font-mono)',fontWeight:600,color:'var(--text-strong)'}}>{VOLUMES.length}</span></span>{sep}
          <span>En erreur : <span style={{fontWeight:600,color:all.some(c=>c.st==='restarting')?'#e5b454':'var(--text-strong)'}}>{all.filter(c=>c.st==='restarting').length}</span></span>
          <span style={{flex:1}}></span>
          <button onClick={()=>flash('docker system prune : 1,2 Go libérés')} style={{padding:0,border:0,background:'transparent',cursor:'pointer',font:'500 13px/1 var(--font-sans)',color:'#a78bfa'}}>Nettoyer les ressources inutilisées</button>
        </div>
        {all.length>0&&<div style={{display:'grid',gridTemplateColumns:'repeat(3,minmax(0,1fr))',gap:12}}>
          <Tl t="CPU des conteneurs" v={cpuT} u="%" sp={cpuT}/>
          <Tl t="Mémoire utilisée" v={(memT/1024).toFixed(1).replace('.',',')} u="Go" sp={memT/100}/>
          <Tl t="Conteneurs en marche" v={run.length} u={'/ '+all.length} bar={Math.round(run.length/all.length*100)}/>
        </div>}
      </div>})()}
    {groups.length===0?<Empty onCompose={t=>flash(t+'…')}/>:<>
      <div style={{display:'flex',alignItems:'center',gap:12,paddingLeft:12,marginTop:6}}>
        {window.CipheraDesignSystem.Tabs?React.createElement(window.CipheraDesignSystem.Tabs,{tabs:TABS.map(t=>({value:t,label:t,count:t==='Conteneurs'?all.length:t==='Images'?IMAGES.length:VOLUMES.length})),value:tab,onChange:setTab}):null}
        <span style={{flex:1}}></span>
        <div style={{display:'flex',alignItems:'center',gap:8,height:32,width:220,padding:'0 10px',borderRadius:8,border:'1px solid var(--border-default)'}}><Icon name="search" size={13} color="var(--text-subtle)"/><input value={q} onChange={e=>setQ(e.target.value)} placeholder="Filtrer…" style={{flex:1,minWidth:0,background:'transparent',border:0,outline:'none',color:'var(--text-strong)',font:'400 12px/1 var(--font-sans)'}}/></div>
      </div>
      {tab==='Conteneurs'&&<div className="log-scroll" style={{overflowX:'auto'}}><div style={{display:'flex',flexDirection:'column',gap:12,minWidth:820}}>
        {groups.map(g=>{const items=g.items.filter(c=>(c.n+c.img).toLowerCase().includes(q.toLowerCase()));if(!items.length)return null;const f=fold[g.stack];const run=g.items.filter(c=>c.st==='running').length;
          return <div key={g.stack} style={{...box,overflow:'hidden'}}>
            <button onClick={()=>setFold(x=>({...x,[g.stack]:!f}))} style={{display:'flex',alignItems:'center',gap:10,width:'100%',padding:'13px 16px',border:0,borderBottom:f?'none':line,background:'rgba(255,255,255,.015)',cursor:'pointer',textAlign:'left'}}>
              <Icon name={f?'chevron-right':'chevron-down'} size={14} color="var(--text-muted)"/><Icon name={g.stack==='Système'?'cpu':'layers'} size={14} color="var(--text-muted)"/>
              <span style={{font:'600 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{g.stack}</span><span style={{...lbl,fontSize:11}}>{run}/{g.items.length} en marche</span>
              <span style={{flex:1}}></span>{g.stack!=='Système'&&<span style={{...lbl,fontSize:11,fontFamily:'var(--font-mono)'}}>docker-compose.yml</span>}
            </button>
            {!f&&<><div style={{display:'grid',gridTemplateColumns:COLS,gap:16,padding:'9px 12px 9px 18px',...lbl,fontSize:11}}><span>Conteneur</span><span>Image</span><span>CPU</span><span>Mémoire</span><span>Ports</span><span>Uptime</span><span></span></div>
              {items.map((c,i)=><CRow key={c.id} c={c} first={false} act={act}/>)}</>}
          </div>})}
      </div></div>}
      {tab==='Images'&&<div style={{...box,overflow:'hidden'}}>{IMAGES.filter(x=>x[0].includes(q)).map(([n,t,sz,w,used],i)=><div key={i} style={{display:'grid',gridTemplateColumns:'minmax(0,2fr) 120px 100px 110px 120px 40px',gap:16,alignItems:'center',minHeight:48,padding:'0 12px 0 18px',borderTop:i?line:'none'}}><span style={{display:'flex',alignItems:'center',gap:10,font:'400 13px/1 var(--font-mono)',color:'var(--text-strong)',minWidth:0}}><Icon name="package" size={14} color="var(--text-muted)"/><span style={{overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'}}>{n}</span></span><span style={{font:'400 12px/1 var(--font-mono)',color:'var(--text-muted)'}}>{t}</span><span style={lbl}>{sz}</span><span style={lbl}>{w}</span><span style={{...lbl,color:used?'var(--text-body)':'#fbbf24'}}>{used?used+' conteneur'+(used>1?'s':''):'Inutilisée'}</span><IconButton size={28} variant="ghost" icon="trash-2" label="Supprimer" onClick={()=>flash('Image '+n+':'+t+' supprimée')}/></div>)}</div>}
      {tab==='Volumes'&&<div style={{...box,overflow:'hidden'}}>{VOLUMES.map(([n,sz,u],i)=><div key={n} style={{display:'grid',gridTemplateColumns:'minmax(0,2fr) 100px minmax(0,1fr) 40px',gap:16,alignItems:'center',minHeight:48,padding:'0 12px 0 18px',borderTop:i?line:'none'}}><span style={{display:'flex',alignItems:'center',gap:10,font:'400 13px/1 var(--font-mono)',color:'var(--text-strong)'}}><Icon name="hard-drive" size={14} color="var(--text-muted)"/>{n}</span><span style={lbl}>{sz}</span><span style={{font:'400 12px/1 var(--font-mono)',color:'var(--text-muted)'}}>{u}</span><IconButton size={28} variant="ghost" icon="download" label="Sauvegarder" onClick={()=>flash('Sauvegarde de '+n+' lancée')}/></div>)}</div>}
    </>}
    <div style={{...box,overflow:'hidden'}}>
      <button onClick={()=>setFaq(f=>!f)} style={{display:'flex',alignItems:'center',width:'100%',padding:'16px 18px',border:0,background:'transparent',cursor:'pointer',font:'600 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>Articles connexes<span style={{flex:1}}></span><Icon name={faq?'chevron-up':'chevron-down'} size={15} color="var(--text-muted)"/></button>
      {faq&&<div style={{borderTop:line}}>{['Déployer un docker-compose existant','Limiter le CPU et la mémoire d\'un conteneur','Persister les données avec des volumes','Lire les logs et ouvrir un shell'].map((t,i)=><a key={t} href="#" onClick={e=>e.preventDefault()} style={{display:'flex',alignItems:'center',gap:10,padding:'12px 18px',borderTop:i?line:'none',font:'400 13px/1 var(--font-sans)',color:'var(--text-body)',textDecoration:'none'}}><Icon name="file-text" size={14} color="var(--text-muted)"/>{t}<span style={{flex:1}}></span><Icon name="arrow-up-right" size={13} color="var(--text-subtle)"/></a>)}</div>}
    </div>
    {logs&&<LogsModal c={logs} onClose={()=>setLogs(null)}/>}
    {cf&&(()=>{const {Modal}=window.CipheraDesignSystem;const M={stop:['Arrêter '+cf.c.n+' ?','Le conteneur reçoit SIGTERM puis SIGKILL après 10 s. Les requêtes en cours seront interrompues.','Arrêter','square'],restart:['Redémarrer '+cf.c.n+' ?','Le conteneur sera arrêté puis relancé avec la même configuration.','Redémarrer','rotate-ccw'],rm:['Supprimer '+cf.c.n+' ?','Le conteneur est supprimé définitivement. Les volumes nommés sont conservés.','Supprimer','trash-2']}[cf.k];return <Modal title={M[0]} description={M[1]} onClose={()=>setCf(null)} width={460} footer={<><Button onClick={()=>setCf(null)}>Annuler</Button><Button variant="inverse" icon={M[3]} onClick={()=>{act(cf.k,cf.c,true);setCf(null)}}>{M[2]}</Button></>}><div style={{display:'flex',alignItems:'center',gap:12,padding:'12px 14px',borderRadius:12,background:'var(--surface-raised)'}}><Icon name="box" size={16} color="var(--text-muted)"/><span style={{display:'flex',flexDirection:'column',gap:6,minWidth:0}}><span style={{font:'500 13px/1 var(--font-mono)',color:'var(--text-strong)'}}>{cf.c.n}</span><span style={{font:'400 12px/1 var(--font-mono)',color:'var(--text-subtle)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{cf.c.img}</span></span></div></Modal>})()}
    {toast&&<div style={{position:'fixed',bottom:24,right:24,zIndex:80,padding:'10px 14px',borderRadius:'var(--radius-md)',background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)',font:'400 12px/1 var(--font-sans)',color:'var(--text-strong)',display:'flex',alignItems:'center',gap:8}}><Icon name="check" size={12} color="var(--green-500)"/>{toast}</div>}
  </div>;
}
window.DockerPage=DockerPage;
})();
