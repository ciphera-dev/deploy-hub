(()=>{
const {Button,IconButton,Badge,Icon,StatusDot,Modal}=window.CipheraDesignSystem;
const hair='1px solid var(--border-subtle)';
const lbl={font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'};
const mono=(v,c)=><span style={{fontFamily:'var(--font-mono)',fontSize:12,color:c}}>{v}</span>;
const card={borderRadius:'var(--radius-xl)',border:'1px solid var(--border-default)',background:'var(--surface-app)'};
const TYPE_IC={Application:'globe',Worker:'cog','Base de données':'database','File de tâches':'list-ordered',Cache:'zap'};

function Spark({data,h=28,hl}){const max=Math.max(...data);return <div style={{display:'flex',alignItems:'flex-end',gap:2,height:h}}>{data.map((v,i)=><span key={i} style={{flex:1,height:Math.max(2,v/max*h),borderRadius:1,background:hl&&hl(v,i)?'var(--red-500)':i===data.length-1?'var(--gray-900)':'var(--viz-3)'}}></span>)}</div>}
function Bar({v,warn=80}){return <span style={{display:'block',height:4,borderRadius:99,background:'var(--viz-4)',overflow:'hidden'}}><span style={{display:'block',height:'100%',width:v+'%',borderRadius:99,background:v>=warn?'#e5b454':'var(--viz-1)'}}></span></span>}
function SIcon({st,size=22}){const m={ok:['check','#0b0b0b','var(--green-500)'],pending:['loader','var(--text-strong)','var(--surface-selected)'],error:['x','#fff','var(--red-500)']}[st]||['minus','var(--text-muted)','var(--surface-selected)'];return <span style={{width:size,height:size,borderRadius:99,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:m[2]}}><Icon name={m[0]} size={size*.5} color={m[1]} style={st==='pending'?{animation:'spin 1.2s linear infinite'}:undefined}/></span>}
const metrics=s=>{const h=[...s.n].reduce((a,c)=>a+c.charCodeAt(0),0);return {cpu:s.s[0]==='error'?0:12+h%45,mem:s.s[0]==='error'?0:25+h%50,memTxt:s.t==='Base de données'?'1,1 Go':(180+h%300)+' Mo'}};

function ProjectOverview({P,M,S,deps,go}){
  const req=[42,38,35,31,29,33,41,55,68,74,79,82,80,77,81,85,88,84,79,72,66,58,51,47];
  const lat=[148,152,150,155,149,160,171,158,152,149,151,190,176,160,154,150,148,151,149,146,152,155,149,182];
  const err=[0,0,1,0,0,0,0,2,0,0,0,7,4,0,0,1,0,0,0,0,0,0,0,1];
  const up=Array.from({length:30},(_,i)=>i===17?96:100);
  const KPI=[['Requêtes (24 h)','1,24 M',req,'+8,2 %'],['Latence p95','182 ms',lat,'+4 ms'],["Taux d'erreur",'0,04 %',err,'−0,01 pt'],['Disponibilité (30 j)','99,98 %',up,'1 incident']];
  return <div style={{padding:'24px 32px 40px 34px',display:'flex',flexDirection:'column',gap:20}}>
    <div style={{...card,overflow:'hidden'}}>
      <div style={{display:'flex',alignItems:'flex-start',gap:16,padding:'22px 22px 20px'}}>
        <SIcon st={P.status[0]} size={36}/>
        <div style={{display:'flex',flexDirection:'column',gap:10,flex:1,minWidth:0}}>
          <div style={{display:'flex',alignItems:'center',gap:10}}><span style={lbl}>Production</span><Badge tone="positive">Actuel</Badge></div>
          <a href="#" style={{display:'flex',alignItems:'center',gap:8,font:'500 22px/1.1 var(--font-sans)',color:'var(--text-strong)',letterSpacing:'-0.01em'}}>{M.domain}<Icon name="arrow-up-right" size={16} color="var(--text-muted)"/></a>
          <span style={{font:'400 13px/1.4 var(--font-sans)',color:'var(--text-muted)'}}>{M.msg}</span>
        </div>
        <div style={{display:'flex',gap:6}}><Button icon="scroll-text" onClick={()=>go('Logs')}>Logs</Button><Button icon="history" onClick={()=>go('Déploiements')}>Historique</Button></div>
      </div>
      <div style={{display:'grid',gridTemplateColumns:'repeat(5,minmax(0,1fr))',borderTop:hair}}>
        {[['Commit',<span style={{display:'flex',alignItems:'center',gap:6}}><Icon name="git-commit-horizontal" size={13} color="var(--text-subtle)"/>{mono(M.commit,'var(--text-strong)')}</span>],['Branche',<span style={{display:'flex',alignItems:'center',gap:6}}><Icon name="git-branch" size={13} color="var(--text-subtle)"/>{mono(P.branch,'var(--text-strong)')}</span>],['Auteur',M.author],['Build',P.build+' · '+M.lang],['Déployé',P.updated+' · '+M.dur]].map(([k,v],i)=><div key={k} style={{display:'flex',flexDirection:'column',gap:10,padding:'16px 22px',borderLeft:i?hair:'none',minWidth:0}}>
          <span style={lbl}>{k}</span><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{v}</span>
        </div>)}
      </div>
    </div>
    <div style={{display:'grid',gridTemplateColumns:'repeat(4,minmax(0,1fr))',gap:12}}>
      {KPI.map(([k,v,data,d],i)=><div key={k} style={{...card,padding:'16px 18px',display:'flex',flexDirection:'column',gap:14}}>
        <div style={{display:'flex',justifyContent:'space-between',alignItems:'baseline'}}><span style={lbl}>{k}</span><span style={{...lbl,fontSize:11}}>{d}</span></div>
        <span style={{font:'500 22px/1 var(--font-sans)',color:'var(--text-strong)',letterSpacing:'-0.01em'}}>{v}</span>
        <Spark data={data} hl={i===2?(v)=>v>3:i===3?(v)=>v<100:null}/>
      </div>)}
    </div>
    <div style={{display:'grid',gridTemplateColumns:'minmax(0,1fr) minmax(0,1fr)',gap:12}}>
      <div style={{...card,overflow:'hidden'}}>
        <div style={{display:'flex',alignItems:'center',justifyContent:'space-between',padding:'16px 18px'}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>Services</span><Button size="sm" variant="ghost" iconRight="arrow-right" onClick={()=>go('Services')}>Tout voir</Button></div>
        {S.map(s=>{const m=metrics(s);return <div key={s.n} style={{display:'grid',gridTemplateColumns:'26px minmax(0,1fr) 90px 60px',gap:12,alignItems:'center',padding:'12px 18px',borderTop:hair}}>
          <span style={{width:26,height:26,borderRadius:7,display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-selected)'}}><Icon name={TYPE_IC[s.t]||'box'} size={13} color="var(--text-body)"/></span>
          <div style={{display:'flex',flexDirection:'column',gap:5,minWidth:0}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{s.n}</span><span style={{...lbl,fontSize:11}}>{s.img}</span></div>
          <div style={{display:'flex',flexDirection:'column',gap:6}}><span style={{...lbl,fontSize:11,fontFamily:'var(--font-mono)'}}>CPU {m.cpu}%</span><Bar v={m.cpu}/></div>
          <span style={{justifySelf:'end'}}><StatusDot status={s.s[0]} label={s.rep}/></span>
        </div>})}
      </div>
      <div style={{...card,overflow:'hidden'}}>
        <div style={{display:'flex',alignItems:'center',justifyContent:'space-between',padding:'16px 18px'}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>Activité récente</span><Button size="sm" variant="ghost" iconRight="arrow-right" onClick={()=>go('Déploiements')}>Déploiements</Button></div>
        {deps.slice(0,4).map((d,i)=><div key={d.id} onClick={()=>go('Déploiements')} style={{display:'grid',gridTemplateColumns:'22px minmax(0,1fr) auto',gap:12,alignItems:'center',padding:'12px 18px',borderTop:hair,cursor:'pointer'}}>
          <SIcon st={d.s[0]}/>
          <div style={{display:'flex',flexDirection:'column',gap:5,minWidth:0}}><span style={{font:'500 13px/1.1 var(--font-sans)',color:'var(--text-strong)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{d.msg}</span><span style={{...lbl,fontSize:11}}>{d.s[1]}</span></div>
          <div style={{display:'flex',flexDirection:'column',gap:5,alignItems:'flex-end'}}>{mono('#'+d.id,'var(--text-muted)')}<span style={{...lbl,fontSize:11}}>{d.when}</span></div>
        </div>)}
      </div>
    </div>
  </div>;
}

function ServiceCard({s,onLogs,flash,onScale}){
  const m=metrics(s);const [cur,max]=s.rep.split('/').map(x=>parseInt(x));const stateless=s.t==='Application'||s.t==='Worker';
  const [menu,setMenu]=React.useState(false);const [busy,setBusy]=React.useState(false);
  React.useEffect(()=>{if(!menu)return;const f=()=>setMenu(false);setTimeout(()=>document.addEventListener('click',f),0);return()=>document.removeEventListener('click',f)},[menu]);
  const restart=()=>{setBusy(true);flash('Redémarrage de '+s.n+'…');setTimeout(()=>setBusy(false),1600)};
  const host=s.n+'.internal'+(s.port&&s.port!=='—'?':'+s.port:'');
  return <div style={{...card,display:'flex',flexDirection:'column'}}>
    <div style={{display:'flex',alignItems:'flex-start',gap:12,padding:'18px 18px 16px'}}>
      <span style={{width:36,height:36,borderRadius:10,display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-selected)',flex:'none'}}><Icon name={TYPE_IC[s.t]||'box'} size={17} color="var(--text-strong)"/></span>
      <div style={{display:'flex',flexDirection:'column',gap:7,flex:1,minWidth:0}}>
        <div style={{display:'flex',alignItems:'center',gap:8}}><span style={{font:'500 15px/1 var(--font-sans)',color:'var(--text-strong)'}}>{s.n}</span><Badge>{s.t}</Badge></div>
        <span style={{...lbl,display:'flex',alignItems:'center',gap:6}}><Icon name={s.img.startsWith('Railpack')?'package':s.img.startsWith('Dockerfile')?'file-code':'container'} size={12}/>{s.img}</span>
      </div>
      <div style={{position:'relative'}}>
        <IconButton size={28} variant="ghost" icon="ellipsis" label="Actions" onClick={()=>setMenu(x=>!x)}/>
        {menu&&<div style={{position:'absolute',top:32,right:0,zIndex:30,width:200,padding:4,background:'var(--surface-tooltip)',borderRadius:'var(--radius-lg)',boxShadow:'var(--shadow-tooltip)'}}>
          {[['terminal','Ouvrir un terminal',()=>flash('Session shell ouverte sur '+s.n+'-1')],['sliders-horizontal','Ressources & limites',()=>flash('Limites : 1 vCPU · 512 Mo')],['square','Arrêter',()=>flash(s.n+' arrêté')]].map(([ic,t,f])=><button key={t} onClick={f} onMouseEnter={e=>e.currentTarget.style.background='var(--surface-selected)'} onMouseLeave={e=>e.currentTarget.style.background='transparent'} style={{display:'flex',alignItems:'center',gap:10,width:'100%',padding:'8px 10px',border:0,borderRadius:6,background:'transparent',cursor:'pointer',color:'var(--text-body)',font:'400 13px/1 var(--font-sans)'}}><Icon name={ic} size={14}/>{t}</button>)}
        </div>}
      </div>
    </div>
    <div style={{display:'flex',alignItems:'center',gap:10,padding:'0 18px 16px'}}>
      <StatusDot status={busy?'pending':s.s[0]} label={busy?'Redémarrage…':s.s[1]}/>
      <span style={{flex:1}}></span>
      <span style={{display:'flex',alignItems:'center',gap:6,height:24,padding:'0 8px',borderRadius:6,background:'var(--surface-raised)',maxWidth:'60%'}}><Icon name="network" size={11} color="var(--text-subtle)"/><span style={{font:'400 11px/1 var(--font-mono)',color:'var(--text-muted)',overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'}}>{host}</span></span>
    </div>
    <div style={{display:'grid',gridTemplateColumns:'1fr 1fr',gap:16,padding:'16px 18px',borderTop:hair}}>
      {[['CPU',m.cpu+' %',m.cpu],['Mémoire',m.memTxt,m.mem]].map(([k,v,p])=><div key={k} style={{display:'flex',flexDirection:'column',gap:8}}><div style={{display:'flex',justifyContent:'space-between'}}><span style={lbl}>{k}</span><span style={{font:'500 12px/1 var(--font-mono)',color:'var(--text-strong)'}}>{v}</span></div><Bar v={p}/></div>)}
    </div>
    <div style={{display:'flex',alignItems:'center',gap:12,padding:'12px 12px 12px 18px',borderTop:hair,marginTop:'auto'}}>
      <span style={lbl}>Réplicas</span>
      <div style={{display:'flex',gap:3}}>{Array.from({length:Math.max(max,1)},(_,i)=><span key={i} style={{width:8,height:8,borderRadius:2,background:i<cur?'var(--green-500)':'var(--viz-3)'}}></span>)}</div>
      {stateless?<div style={{display:'flex',alignItems:'center',borderRadius:'var(--radius-md)',border:'1px solid var(--border-strong)',background:'var(--surface-control)',height:26}}>
        <button onClick={()=>onScale(-1)} disabled={max<=1} style={{width:24,height:24,border:0,background:'transparent',color:'var(--text-muted)',cursor:max<=1?'default':'pointer',display:'flex',alignItems:'center',justifyContent:'center',opacity:max<=1?.4:1}}><Icon name="minus" size={12}/></button>
        <span style={{minWidth:18,textAlign:'center',font:'500 12px/1 var(--font-mono)',color:'var(--text-strong)'}}>{max}</span>
        <button onClick={()=>onScale(1)} style={{width:24,height:24,border:0,background:'transparent',color:'var(--text-muted)',cursor:'pointer',display:'flex',alignItems:'center',justifyContent:'center'}}><Icon name="plus" size={12}/></button>
      </div>:<span style={{...lbl,fontSize:11}}>{cur}/{max}</span>}
      <span style={{flex:1}}></span>
      <IconButton size={28} variant="ghost" icon="scroll-text" label="Logs" onClick={onLogs}/>
      <IconButton size={28} variant="ghost" icon="rotate-ccw" label="Redémarrer" onClick={restart}/>
    </div>
  </div>;
}

function ProjectServices({S,go}){
  const [list,setList]=React.useState(S);
  const [add,setAdd]=React.useState(false);
  const [toast,setToast]=React.useState('');
  const flash=m=>{setToast(m);clearTimeout(flash.t);flash.t=setTimeout(()=>setToast(''),2000)};
  const scale=(n,d)=>setList(l=>l.map(s=>{if(s.n!==n)return s;const max=Math.max(1,parseInt(s.rep.split('/')[1])+d);flash(n+' : '+max+' réplica'+(max>1?'s':''));return {...s,rep:max+' / '+max}}));
  const KINDS=[['database','PostgreSQL','Base relationnelle avec sauvegardes'],['zap','Redis','Cache et pub/sub en mémoire'],['cog','Worker','Processus de fond depuis ce dépôt'],['container','Image Docker','N\'importe quelle image publique ou privée'],['clock','Tâche planifiée','Commande lancée selon un cron'],['hard-drive','Volume','Stockage persistant monté']];
  return <div style={{padding:'24px 32px 40px 34px',display:'flex',flexDirection:'column',gap:16}}>
    <div style={{display:'flex',alignItems:'center',gap:12}}>
      <span style={{font:'400 12px/1.4 var(--font-sans)',color:'var(--text-subtle)'}}>Les services communiquent sur le réseau privé du projet via <span style={{fontFamily:'var(--font-mono)',color:'var(--text-body)'}}>nom.internal</span>.</span>
      <span style={{flex:1}}></span>
      <Button variant="inverse" icon="plus" onClick={()=>setAdd(true)}>Ajouter un service</Button>
    </div>
    <div style={{display:'grid',gridTemplateColumns:'repeat(auto-fill,minmax(300px,1fr))',gap:12}}>
      {list.map(s=><ServiceCard key={s.n} s={s} flash={flash} onLogs={()=>go('Logs')} onScale={d=>scale(s.n,d)}/>)}
      <button onClick={()=>setAdd(true)} style={{...card,minHeight:220,borderStyle:'dashed',borderColor:'var(--border-strong)',background:'transparent',display:'flex',flexDirection:'column',alignItems:'center',justifyContent:'center',gap:10,cursor:'pointer',color:'var(--text-muted)'}}>
        <span style={{width:36,height:36,borderRadius:99,border:'1px dashed var(--gray-500)',display:'flex',alignItems:'center',justifyContent:'center'}}><Icon name="plus" size={15}/></span>
        <span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-body)'}}>Nouveau service</span>
        <span style={lbl}>Base de données, cache, worker, image…</span>
      </button>
    </div>
    {add&&<Modal title="Ajouter un service" description="Le service rejoint le réseau privé du projet." onClose={()=>setAdd(false)} width={600} footer={<Button onClick={()=>setAdd(false)}>Annuler</Button>}>
      <div style={{display:'grid',gridTemplateColumns:'1fr 1fr',gap:8}}>
        {KINDS.map(([ic,t,d])=><button key={t} onClick={()=>{const n=t==='PostgreSQL'?'postgres-2':t.toLowerCase().split(' ')[0];setList(l=>[...l,{n,t:t==='PostgreSQL'?'Base de données':t==='Redis'?'Cache':t==='Worker'?'Worker':'Application',img:t==='PostgreSQL'?'postgres:17':t==='Redis'?'redis:7':t==='Worker'?'Railpack · Node 22':'image',s:['pending','Démarrage'],rep:'1 / 1',port:t==='Redis'?'6379':t==='PostgreSQL'?'5432':'—'}]);setAdd(false);flash(t+' ajouté')}} onMouseEnter={e=>e.currentTarget.style.background='var(--surface-raised)'} onMouseLeave={e=>e.currentTarget.style.background='var(--surface-control)'} style={{display:'flex',alignItems:'center',gap:12,padding:'14px',borderRadius:'var(--radius-lg)',border:'1px solid var(--border-strong)',background:'var(--surface-control)',cursor:'pointer',textAlign:'left'}}>
          <span style={{width:32,height:32,borderRadius:8,display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-selected)',flex:'none'}}><Icon name={ic} size={15} color="var(--text-strong)"/></span>
          <span style={{display:'flex',flexDirection:'column',gap:5}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{t}</span><span style={{...lbl,fontSize:11}}>{d}</span></span>
        </button>)}
      </div>
    </Modal>}
    {toast&&<div style={{position:'fixed',bottom:24,right:24,zIndex:60,padding:'10px 14px',borderRadius:'var(--radius-md)',background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)',font:'400 12px/1 var(--font-sans)',color:'var(--text-strong)',display:'flex',alignItems:'center',gap:8}}><Icon name="check" size={12} color="var(--green-500)"/>{toast}</div>}
  </div>;
}
Object.assign(window,{ProjectOverview,ProjectServices});
})();
