(()=>{
const {Button,IconButton,Select,Icon,StatusDot}=window.CipheraDesignSystem;
const hair='1px solid var(--border-subtle)';
const LV={info:{c:'var(--text-muted)',bg:'transparent',t:'INFO',n:'Info'},warn:{c:'#e5b454',bg:'rgba(229,180,84,.10)',t:'WARN',n:'Warn'},error:{c:'#f87171',bg:'rgba(239,68,68,.12)',t:'ERR',n:'Erreur'},debug:{c:'var(--text-faint)',bg:'transparent',t:'DBG',n:'Debug'}};
const SAMPLES={
  web:[['info','GET / 200 12ms'],['info','GET /api/projects 200 34ms'],['info','GET /_next/static/chunks/app.js 304 2ms'],['debug','cache hit key=dashboard:alex'],['warn','Slow query 812ms: SELECT * FROM deployments ORDER BY created_at'],['info','POST /api/deploy 202 48ms'],['error','Unhandled rejection: fetch failed (ECONNRESET) upstream=billing-api'],['info','GET /health 200 1ms']],
  worker:[['info','job deploy#4821 picked (SKIP LOCKED)'],['info','job deploy#4821 step=build status=running'],['debug','buildkit session 7f3a opened'],['info','job email#9912 sent to lina@ciphera.dev'],['warn','retrying job webhook#331 (attempt 2/5)'],['info','job deploy#4821 done in 96s']],
  postgres:[['info','checkpoint starting: time'],['info','checkpoint complete: wrote 412 buffers (2.5%)'],['warn','duration: 812.4 ms statement: SELECT * FROM deployments'],['info','connection authorized: user=ciphera database=web'],['debug','autovacuum: processing table public.events']],
};
const CONTAINERS=[
  {id:'web-1',svc:'web',img:'web:4821',st:'ok',up:'4 min',cpu:18,mem:'212 Mo',col:'#b5b5b5'},{id:'web-2',svc:'web',img:'web:4821',st:'ok',up:'4 min',cpu:22,mem:'236 Mo',col:'#8fa3b8'},
  {id:'worker-1',svc:'worker',img:'web:4821',st:'ok',up:'4 min',cpu:41,mem:'388 Mo',col:'#7fae99'},{id:'postgres-1',svc:'postgres',img:'postgres:16',st:'ok',up:'12 j',cpu:9,mem:'1,1 Go',col:'#a395c4'},
  {id:'web-1·4818',svc:'web',img:'web:4818',st:'off',up:'arrêté',cpu:0,mem:'—',col:'#6f6f6f'},
];
const ALL='all';
const pad=n=>String(n).padStart(2,'0');
let ctr=0;
function mk(cid,base){
  const C=CONTAINERS.find(c=>c.id===cid)||CONTAINERS[0];const s=SAMPLES[C.svc];const [lv,msg]=s[Math.floor(Math.random()*s.length)];
  const d=base||new Date();return {id:++ctr,t:d.getTime(),ts:pad(d.getHours())+':'+pad(d.getMinutes())+':'+pad(d.getSeconds())+'.'+String(d.getMilliseconds()).padStart(3,'0'),date:d.toISOString(),c:cid,lv,msg};
}
function seed(){const out=[];const now=Date.now();for(let i=220;i>0;i--){const c=CONTAINERS[Math.floor(Math.random()*4)].id;out.push(mk(c,new Date(now-i*2300)))}return out}

function Histogram({logs}){
  const B=60;if(!logs.length)return <div style={{height:44}}></div>;
  const t0=logs[0].t,t1=logs[logs.length-1].t||t0+1,span=Math.max(1,t1-t0);
  const bins=Array.from({length:B},()=>({n:0,e:0,w:0}));
  logs.forEach(l=>{const i=Math.min(B-1,Math.floor((l.t-t0)/span*B));bins[i].n++;if(l.lv==='error')bins[i].e++;if(l.lv==='warn')bins[i].w++});
  const max=Math.max(1,...bins.map(b=>b.n));
  return <div style={{display:'flex',alignItems:'flex-end',gap:2,height:44}}>{bins.map((b,i)=><div key={i} title={b.n+' lignes'} style={{flex:1,height:Math.max(2,b.n/max*44),display:'flex',flexDirection:'column',justifyContent:'flex-start',borderRadius:2,overflow:'hidden',background:'var(--viz-3)'}}>
    {b.e>0&&<span style={{height:(b.e/b.n*100)+'%',background:'var(--red-500)',minHeight:2}}></span>}
    {b.w>0&&<span style={{height:(b.w/b.n*100)+'%',background:'#e5b454',minHeight:2}}></span>}
  </div>)}</div>;
}

function LogRow({l,i,showC,wrap,ts,hl,onCopy}){
  const [h,setH]=React.useState(false);const L=LV[l.lv];const C=CONTAINERS.find(c=>c.id===l.c);
  return <div onMouseEnter={()=>setH(true)} onMouseLeave={()=>setH(false)} style={{position:'relative',display:'flex',alignItems:'flex-start',gap:14,padding:'3px 16px 3px 0',minHeight:24,background:h?'var(--surface-raised)':(l.lv==='error'?'rgba(239,68,68,.05)':'transparent'),boxShadow:l.lv==='error'?'inset 2px 0 0 #ef4444':l.lv==='warn'?'inset 2px 0 0 #e5b454':'none'}}>
    <span style={{width:44,flex:'none',textAlign:'right',color:'var(--gray-500)',userSelect:'none'}}>{i+1}</span>
    {ts&&<span style={{color:'var(--text-faint)',flex:'none'}}>{l.ts}</span>}
    <span style={{flex:'none',width:36,height:18,display:'flex',alignItems:'center',justifyContent:'center',borderRadius:4,background:L.bg,color:L.c,fontSize:10.5,fontWeight:500,letterSpacing:'.03em',marginTop:1}}>{L.t}</span>
    {showC&&<span style={{flex:'none',width:84,display:'flex',alignItems:'center',gap:6,color:'var(--text-muted)',overflow:'hidden',whiteSpace:'nowrap'}}><span style={{width:6,height:6,borderRadius:2,background:C?.col,flex:'none'}}></span>{l.c}</span>}
    <span style={{flex:1,minWidth:0,color:l.lv==='error'?'#fca5a5':l.lv==='debug'?'var(--text-subtle)':'var(--text-body)',whiteSpace:wrap?'pre-wrap':'pre',overflow:wrap?'visible':'hidden',textOverflow:'ellipsis',wordBreak:wrap?'break-all':'normal'}}>{hl(l.msg)}</span>
    {h&&<button onClick={()=>onCopy(l)} title="Copier la ligne" style={{position:'absolute',right:10,top:2,width:22,height:22,display:'flex',alignItems:'center',justifyContent:'center',borderRadius:5,border:'1px solid var(--border-strong)',background:'var(--surface-control)',color:'var(--text-muted)',cursor:'pointer',padding:0}}><Icon name="copy" size={11}/></button>}
  </div>;
}

function ProjectLogs({project}){
  const [logs,setLogs]=React.useState(seed);
  const [sel,setSel]=React.useState(ALL);
  const [lv,setLv]=React.useState('all');
  const [q,setQ]=React.useState('');
  const [live,setLive]=React.useState(true);
  const [wrap,setWrap]=React.useState(false);
  const [ts,setTs]=React.useState(true);
  const [range,setRange]=React.useState('15 dernières minutes');
  const [toast,setToast]=React.useState('');
  const [atBottom,setAtBottom]=React.useState(true);
  const box=React.useRef(null);
  const selC=CONTAINERS.find(c=>c.id===sel);
  React.useEffect(()=>{if(!live||selC?.st==='off')return;const t=setInterval(()=>{const pool=sel===ALL?CONTAINERS.slice(0,4).map(c=>c.id):[sel];const n=1+Math.floor(Math.random()*2);setLogs(l=>[...l,...Array.from({length:n},()=>mk(pool[Math.floor(Math.random()*pool.length)]))].slice(-1500))},800);return()=>clearInterval(t)},[live,sel]);
  const scoped=logs.filter(l=>sel===ALL||l.c===sel);
  const shown=scoped.filter(l=>(lv==='all'?l.lv!=='debug':l.lv===lv)&&(!q||(l.msg+l.c).toLowerCase().includes(q.toLowerCase())));
  React.useLayoutEffect(()=>{const b=box.current;if(b&&atBottom)b.scrollTop=b.scrollHeight},[shown.length,atBottom,sel,lv]);
  const onScroll=()=>{const b=box.current;setAtBottom(b.scrollHeight-b.scrollTop-b.clientHeight<24)};
  const flash=m=>{setToast(m);clearTimeout(flash.t);flash.t=setTimeout(()=>setToast(''),1800)};
  const line=l=>`${l.date} [${LV[l.lv].t}] ${l.c} | ${l.msg}`;
  const download=()=>{const blob=new Blob([shown.map(line).join('\n')+'\n'],{type:'text/plain;charset=utf-8'});const a=document.createElement('a');a.href=URL.createObjectURL(blob);a.download=(project||'projet')+'_'+(sel===ALL?'tous':sel.replace(/\W+/g,'-'))+'_'+new Date().toISOString().slice(0,19).replace(/:/g,'-')+'.txt';document.body.appendChild(a);a.click();a.remove();setTimeout(()=>URL.revokeObjectURL(a.href),1000);flash(shown.length+' lignes téléchargées')};
  const copyAll=()=>{navigator.clipboard&&navigator.clipboard.writeText(shown.map(line).join('\n'));flash(shown.length+' lignes copiées')};
  const counts={info:0,warn:0,error:0,debug:0};scoped.forEach(l=>counts[l.lv]++);
  const hl=t=>{if(!q)return t;const i=t.toLowerCase().indexOf(q.toLowerCase());if(i<0)return t;return <>{t.slice(0,i)}<mark style={{background:'var(--text-strong)',color:'#0b0b0b',borderRadius:2,padding:'0 1px'}}>{t.slice(i,i+q.length)}</mark>{t.slice(i+q.length)}</>};
  const LVTABS=[['all','Tout',counts.info+counts.warn+counts.error],['error','Erreurs',counts.error],['warn','Warnings',counts.warn],['info','Info',counts.info],['debug','Debug',counts.debug]];
  const ctrl={display:'flex',alignItems:'center',height:32,borderRadius:'var(--radius-md)',border:'1px solid var(--border-strong)',background:'var(--surface-control)'};
  const tbtn=(on)=>({display:'flex',alignItems:'center',justifyContent:'center',gap:6,height:30,minWidth:30,padding:'0 8px',border:0,borderRadius:7,cursor:'pointer',background:on?'var(--surface-selected)':'transparent',color:on?'var(--text-strong)':'var(--text-muted)',font:'500 12px/1 var(--font-sans)'});
  return <div style={{display:'grid',gridTemplateColumns:'220px minmax(0,1fr)',gap:20,alignItems:'start'}}>
    <div style={{display:'flex',flexDirection:'column',gap:4,position:'sticky',top:0}}>
      <div style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)',padding:'8px 10px 10px'}}>Conteneurs</div>
      {[{id:ALL},...CONTAINERS].map(C=>{const on=sel===C.id;const isAll=C.id===ALL;return <button key={C.id} onClick={()=>setSel(C.id)} style={{display:'flex',flexDirection:'column',gap:8,padding:'10px 10px',borderRadius:'var(--radius-md)',border:'1px solid '+(on?'var(--border-strong)':'transparent'),cursor:'pointer',textAlign:'left',background:on?'var(--surface-raised)':'transparent',opacity:C.st==='off'&&!on?.6:1}}>
        <span style={{display:'flex',alignItems:'center',gap:8}}>
          {isAll?<Icon name="layers" size={13} color="var(--text-muted)"/>:<span style={{width:8,height:8,borderRadius:2,background:C.col,flex:'none'}}></span>}
          <span style={{flex:1,font:(isAll?'500 13px/1 var(--font-sans)':'500 12px/1 var(--font-mono)'),color:on?'var(--text-strong)':'var(--text-body)'}}>{isAll?'Tous les conteneurs':C.id}</span>
          {!isAll&&<StatusDot status={C.st}/>}
        </span>
        {!isAll&&<span style={{display:'flex',alignItems:'center',gap:10,paddingLeft:16,font:'400 11px/1 var(--font-sans)',color:'var(--text-faint)'}}>
          <span>{C.img}</span><span style={{flex:1}}></span>{C.st==='off'?<span>arrêté</span>:<><span>{C.cpu} %</span><span>{C.mem}</span></>}
        </span>}
        {!isAll&&C.st!=='off'&&<span style={{marginLeft:16,height:2,borderRadius:2,background:'var(--viz-4)',overflow:'hidden'}}><span style={{display:'block',height:'100%',width:C.cpu+'%',background:'var(--viz-1)'}}></span></span>}
      </button>})}
    </div>
    <div style={{display:'flex',flexDirection:'column',gap:14,minWidth:0}}>
      <div style={{display:'flex',alignItems:'center',gap:8}}>
        <div style={{...ctrl,flex:1,minWidth:0,gap:8,padding:'0 10px'}}>
          <Icon name="search" size={14} color="var(--text-subtle)"/>
          <input value={q} onChange={e=>setQ(e.target.value)} placeholder="Rechercher dans les logs" style={{flex:1,minWidth:0,background:'transparent',border:0,outline:'none',color:'var(--text-strong)',font:'400 13px/1 var(--font-sans)'}}/>
          {q?<button onClick={()=>setQ('')} style={{background:'none',border:0,padding:0,cursor:'pointer',color:'var(--text-subtle)',display:'flex'}}><Icon name="x" size={13}/></button>:<span style={{font:'500 10px/1 var(--font-mono)',color:'var(--text-faint)',border:'1px solid var(--border-strong)',borderRadius:4,padding:'3px 5px'}}>/</span>}
        </div>
        <Select icon="clock" options={['5 dernières minutes','15 dernières minutes','1 dernière heure','24 dernières heures','7 derniers jours']} value={range} onChange={setRange}/>
        <Button icon="download" variant="inverse" onClick={download}>Télécharger .txt</Button>
      </div>
      <div style={{borderRadius:'var(--radius-xl)',border:'1px solid var(--border-default)',background:'var(--gray-0)',overflow:'hidden',position:'relative'}}>
        <div style={{display:'flex',alignItems:'center',gap:12,padding:'10px 10px 10px 14px',borderBottom:hair}}>
          <div style={{display:'flex',gap:2,padding:2,borderRadius:9,background:'var(--surface-control)',border:'1px solid var(--border-default)'}}>
            {LVTABS.map(([k,n,c])=><button key={k} onClick={()=>setLv(k)} style={tbtn(lv===k)}>{k!=='all'&&<span style={{width:6,height:6,borderRadius:99,background:k==='error'?'#ef4444':k==='warn'?'#e5b454':k==='info'?'var(--viz-1)':'var(--viz-3)'}}></span>}{n}<span style={{color:'var(--text-faint)',fontFamily:'var(--font-mono)',fontSize:11,fontWeight:400}}>{c}</span></button>)}
          </div>
          <span style={{flex:1}}></span>
          <div style={{display:'flex',gap:2}}>
            <button title="Horodatage" onClick={()=>setTs(v=>!v)} style={tbtn(ts)}><Icon name="clock" size={14}/></button>
            <button title="Retour à la ligne" onClick={()=>setWrap(v=>!v)} style={tbtn(wrap)}><Icon name="wrap-text" size={14}/></button>
            <button title="Copier" onClick={copyAll} style={tbtn(false)}><Icon name="copy" size={14}/></button>
            <button title="Vider" onClick={()=>{setLogs([]);flash('Affichage vidé')}} style={tbtn(false)}><Icon name="eraser" size={14}/></button>
          </div>
          <span style={{width:1,height:20,background:'var(--border-strong)'}}></span>
          <button onClick={()=>setLive(v=>!v)} style={{...tbtn(false),gap:8,padding:'0 10px',color:'var(--text-strong)'}}>
            <span style={{position:'relative',width:8,height:8}}>{live&&selC?.st!=='off'&&<span className="live-ping" style={{position:'absolute',inset:0,borderRadius:99,background:'var(--green-500)'}}></span>}<span style={{position:'absolute',inset:0,borderRadius:99,background:live&&selC?.st!=='off'?'var(--green-500)':'var(--viz-2)'}}></span></span>
            {live?'En direct':'En pause'}<Icon name={live?'pause':'play'} size={12} color="var(--text-muted)"/>
          </button>
        </div>
        <div style={{padding:'14px 16px 10px',borderBottom:hair,display:'flex',flexDirection:'column',gap:8}}>
          <Histogram logs={scoped}/>
          <div style={{display:'flex',justifyContent:'space-between',font:'400 11px/1 var(--font-sans)',color:'var(--text-faint)'}}><span>{scoped[0]?.ts.slice(0,5)||'—'}</span><span>{shown.length} lignes affichées · {sel===ALL?'tous les conteneurs':sel}</span><span>{scoped[scoped.length-1]?.ts.slice(0,5)||'—'}</span></div>
        </div>
        <div ref={box} onScroll={onScroll} className="log-scroll" style={{height:460,overflowY:'auto',padding:'6px 0',font:'400 12px/18px var(--font-mono)'}}>
          {shown.map((l,i)=><LogRow key={l.id} l={l} i={i} showC={sel===ALL} wrap={wrap} ts={ts} hl={hl} onCopy={x=>{navigator.clipboard&&navigator.clipboard.writeText(line(x));flash('Ligne copiée')}}/>)}
          {!shown.length&&<div style={{height:'100%',display:'flex',flexDirection:'column',alignItems:'center',justifyContent:'center',gap:10,fontFamily:'var(--font-sans)'}}>
            <Icon name={selC?.st==='off'?'circle-pause':'search-x'} size={20} color="var(--text-faint)"/>
            <span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-body)'}}>{selC?.st==='off'?'Conteneur arrêté':'Aucun log'}</span>
            <span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'}}>{selC?.st==='off'?'Les logs conservés sont disponibles au téléchargement.':'Modifiez la recherche ou le filtre de niveau.'}</span>
          </div>}
        </div>
        {!atBottom&&<button onClick={()=>setAtBottom(true)} style={{position:'absolute',bottom:16,left:'50%',transform:'translateX(-50%)',display:'flex',alignItems:'center',gap:6,height:30,padding:'0 14px',borderRadius:99,border:0,background:'var(--surface-inverse)',color:'var(--text-inverse)',font:'500 12px/1 var(--font-sans)',cursor:'pointer',boxShadow:'0 8px 24px rgba(0,0,0,.5)'}}><Icon name="arrow-down" size={12}/>Revenir au direct</button>}
        {toast&&<div style={{position:'absolute',top:12,left:'50%',transform:'translateX(-50%)',padding:'8px 12px',borderRadius:'var(--radius-md)',background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)',font:'400 12px/1 var(--font-sans)',color:'var(--text-strong)',display:'flex',alignItems:'center',gap:8}}><Icon name="check" size={12} color="var(--green-500)"/>{toast}</div>}
      </div>
    </div>
  </div>;
}
window.ProjectLogs=ProjectLogs;
})();
