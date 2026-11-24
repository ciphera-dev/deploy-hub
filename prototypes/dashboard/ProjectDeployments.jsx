(()=>{
const {Button,IconButton,Badge,Icon,StatusDot,CodeBlock,Modal,Tabs}=window.CipheraDesignSystem;
const hair='1px solid var(--border-subtle)';
const mono=(v,c)=><span style={{fontFamily:'var(--font-mono)',fontSize:12,color:c}}>{v}</span>;
const lbl={font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'};
const STAGES=['Clonage','Build','Image','Démarrage','Healthcheck','Bascule'];
const LOG=[
  ['cmd','$ ciphera deploy --project ciphera-web',0],['muted','Agent srv-par-01 connecté (QUIC · mTLS)',0],['info','Clonage github.com/ciphera/web@{sha}',0],
  ['info','Railpack : Node 22 détecté, pnpm 9',1],['muted','BuildKit : 14 / 18 couches en cache',1],['info','#4 pnpm install --frozen-lockfile',1],['ok','#4 DONE 6.8s',1],['info','#5 pnpm build',1],['ok','#5 ✓ Compiled successfully',1],
  ['info','Image poussée : registry.ciphera.dev/web:{id}',2],['info','Démarrage de 2 réplicas sur srv-par-01',3],['ok','Healthcheck GET /health → 200',4],['ok','Trafic basculé via Caddy, ancienne version arrêtée',5]];
const DEPS=[
  {id:'4821',sha:'c81e0a9',msg:'feat: graphiques de latence',branch:'main',author:'Lina Moreau',av:'LM',trigger:'push',env:'Production',st:'running',stage:1,when:'il y a 2 min',dur:null,times:[3,null,null,null,null,null]},
  {id:'4818',sha:'a3f91c2',msg:'fix: cache des en-têtes',branch:'main',author:'Alex Martin',av:'AM',trigger:'push',env:'Production',st:'ok',current:true,when:'il y a 4 h',dur:'1 min 36 s',times:[3,62,9,8,4,2]},
  {id:'4815',sha:'e19b44d',msg:'feat: filtre des logs par niveau',branch:'feat/logs',author:'Yanis Benali',av:'YB',trigger:'PR #212',env:'Preview',st:'ok',when:'il y a 6 h',dur:'1 min 22 s',times:[2,58,8,7,4,3],url:'pr-212.preview.ciphera.dev'},
  {id:'4812',sha:'0d77f21',msg:'chore: mise à jour Node 22',branch:'main',author:'Alex Martin',av:'AM',trigger:'manuel',env:'Production',st:'ok',when:'hier',dur:'2 min 05 s',times:[3,88,12,10,9,3]},
  {id:'4809',sha:'5b2e0c8',msg:'revert: bascule CDN',branch:'main',author:'Lina Moreau',av:'LM',trigger:'rollback',env:'Production',st:'cancelled',when:'il y a 2 j',dur:'18 s',times:[3,15,null,null,null,null]},
  {id:'4807',sha:'9fa3c10',msg:'feat: page tarifs',branch:'main',author:'Yanis Benali',av:'YB',trigger:'push',env:'Production',st:'error',when:'il y a 2 j',dur:'41 s',failAt:1,times:[3,38,null,null,null,null]},
];
const ST={running:['pending','En cours'],ok:['ok','Réussi'],error:['error','Échec'],cancelled:['off','Annulé'],queued:['pending','En file']};
const TRIG={push:'git-commit-horizontal',manuel:'mouse-pointer-click',rollback:'undo-2'};

function Avatar({t}){return <span style={{width:20,height:20,borderRadius:99,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-selected)',color:'var(--text-body)',font:'600 9px/1 var(--font-sans)'}}>{t}</span>}

function StatusIcon({st,size=28}){
  const m={running:['loader','var(--text-strong)','var(--surface-selected)'],ok:['check','#0b0b0b','var(--green-500)'],error:['x','#fff','var(--red-500)'],cancelled:['minus','var(--text-muted)','var(--surface-selected)']}[st];
  return <span style={{width:size,height:size,borderRadius:99,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:m[2]}}><Icon name={m[0]} size={size*.5} color={m[1]} style={st==='running'?{animation:'spin 1.2s linear infinite'}:undefined}/></span>;
}

function DeployItem({d,on,onClick,last}){
  const [h,setH]=React.useState(false);
  return <div onClick={onClick} onMouseEnter={()=>setH(true)} onMouseLeave={()=>setH(false)} style={{position:'relative',display:'grid',gridTemplateColumns:'28px minmax(0,1fr)',gap:14,padding:'14px 14px 14px 12px',borderRadius:'var(--radius-lg)',cursor:'pointer',background:on?'var(--surface-raised)':h?'rgba(255,255,255,.02)':'transparent',border:'1px solid '+(on?'var(--border-strong)':'transparent')}}>
    {!last&&<span style={{position:'absolute',left:25,top:44,bottom:-14,width:1,background:'var(--border-default)'}}></span>}
    <StatusIcon st={d.st}/>
    <div style={{display:'flex',flexDirection:'column',gap:8,minWidth:0}}>
      <div style={{display:'flex',alignItems:'center',gap:8,minWidth:0}}>
        <span style={{font:'500 13px/1.2 var(--font-sans)',color:'var(--text-strong)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis',flex:1}}>{d.msg}</span>
        {d.current&&<Badge tone="positive">Actuel</Badge>}
      </div>
      <div style={{display:'flex',alignItems:'center',gap:10,font:'400 11px/1 var(--font-sans)',color:'var(--text-subtle)',minWidth:0,whiteSpace:'nowrap'}}>
        {mono('#'+d.id,'var(--text-muted)')}
        <span style={{display:'flex',alignItems:'center',gap:4}}><Icon name="git-branch" size={11}/>{mono(d.branch)}</span>
        <span style={{display:'flex',alignItems:'center',gap:4}}><Icon name={TRIG[d.trigger]||'git-pull-request'} size={11}/>{d.trigger}</span>
        <span style={{flex:1}}></span><span>{d.when}</span>
      </div>
    </div>
  </div>;
}

function Pipeline({d,stage}){
  const total=d.times.reduce((a,b)=>a+(b||0),0)||1;
  return <div style={{display:'grid',gridTemplateColumns:`repeat(${STAGES.length},minmax(0,1fr))`,gap:4}}>
    {STAGES.map((s,i)=>{
      const done=d.st==='ok'||(d.st==='running'&&i<stage)||(d.st!=='running'&&d.times[i]!=null&&!(d.failAt===i));
      const fail=d.failAt===i;const run=d.st==='running'&&i===stage;const skip=!done&&!fail&&!run;
      const col=fail?'var(--red-500)':done?'var(--viz-1)':run?'var(--gray-900)':'var(--viz-4)';
      return <div key={s} style={{display:'flex',flexDirection:'column',gap:8,minWidth:0}}>
        <div style={{height:4,borderRadius:99,background:'var(--viz-4)',overflow:'hidden'}}><div className={run?'bar-run':''} style={{height:'100%',width:run?'60%':skip?0:'100%',background:col,borderRadius:99}}></div></div>
        <div style={{display:'flex',alignItems:'center',gap:6,font:'500 12px/1 var(--font-sans)',color:skip?'var(--text-faint)':'var(--text-body)'}}>
          {fail?<Icon name="circle-x" size={12} color="var(--red-500)"/>:done?<Icon name="circle-check" size={12} color="var(--text-muted)"/>:run?<Icon name="loader" size={12} style={{animation:'spin 1.2s linear infinite'}}/>:<Icon name="circle-dashed" size={12} color="var(--text-faint)"/>}{s}
        </div>
        <span style={{...lbl,fontSize:11,fontFamily:'var(--font-mono)'}}>{d.times[i]!=null&&!run?d.times[i]+' s':run?'…':'—'}</span>
      </div>})}
  </div>;
}

function ProjectDeployments({project='ciphera-web'}){
  const [deps,setDeps]=React.useState(DEPS);
  const [sel,setSel]=React.useState(DEPS[0].id);
  const [env,setEnv]=React.useState('Tous');
  const [lines,setLines]=React.useState(5);
  const [confirm,setConfirm]=React.useState(null);
  const [toast,setToast]=React.useState('');
  const flash=m=>{setToast(m);clearTimeout(flash.t);flash.t=setTimeout(()=>setToast(''),2000)};
  const run=deps.find(d=>d.st==='running');
  React.useEffect(()=>{if(!run)return;const t=setInterval(()=>setLines(n=>n+1),900);return()=>clearInterval(t)},[run&&run.id]);
  React.useEffect(()=>{if(!run)return;const stage=lines>=LOG.length?6:LOG[Math.min(lines,LOG.length-1)][2];
    if(lines>=LOG.length){setDeps(ds=>ds.map(d=>d.id===run.id?{...d,st:'ok',current:true,dur:'1 min 29 s',when:"à l'instant",times:[3,58,9,8,5,2]}:{...d,current:false}));flash('#'+run.id+' est en ligne');}
    else setDeps(ds=>ds.map(d=>d.id===run.id?{...d,stage}:d));},[lines]);
  const list=deps.filter(d=>env==='Tous'||d.env===env);
  const D=deps.find(d=>d.id===sel)||deps[0];
  const logLines=(D.st==='running'?LOG.slice(0,lines):D.st==='error'?[...LOG.slice(0,6),['error','error: pnpm install a échoué (ERR_PNPM_LOCKFILE)',1],['error','Build interrompu après 41 s',1]]:D.st==='cancelled'?[...LOG.slice(0,5),['muted','Annulé par Lina Moreau',1]]:LOG)
    .map(([tone,t],i)=>({time:'12:04:'+String(31+i*2).padStart(2,'0'),tone,text:t.replace('{sha}',D.sha).replace('{id}',D.id)}));
  const download=()=>{const a=document.createElement('a');a.href=URL.createObjectURL(new Blob([logLines.map(l=>l.time+'  '+l.text).join('\n')+'\n'],{type:'text/plain'}));a.download=project+'_deploy-'+D.id+'.txt';document.body.appendChild(a);a.click();a.remove();flash('Logs téléchargés')};
  const redeploy=()=>{if(run){flash('Un déploiement est déjà en cours');return}const nd={...D,id:String(Math.max(...deps.map(x=>+x.id))+1),st:'running',stage:0,current:false,trigger:'manuel',when:"à l'instant",dur:null,times:[3,null,null,null,null,null],failAt:undefined,author:'Alex Martin',av:'AM'};setDeps(ds=>[nd,...ds]);setSel(nd.id);setLines(1);flash('Déploiement #'+nd.id+' lancé')};
  const cancel=()=>{setDeps(ds=>ds.map(d=>d.id===D.id?{...d,st:'cancelled',dur:'—'}:d));flash('#'+D.id+' annulé')};
  const rollback=()=>{setDeps(ds=>ds.map(d=>({...d,current:d.id===D.id})));setConfirm(null);flash('Rollback vers #'+D.id+' effectué')};
  return <div style={{display:'grid',gridTemplateColumns:'minmax(300px,380px) minmax(0,1fr)',gap:24,alignItems:'start'}}>
    <div style={{display:'flex',flexDirection:'column',gap:12}}>
      <div style={{display:'flex',alignItems:'center',justifyContent:'space-between',paddingLeft:12}}>
        <Tabs tabs={['Tous','Production','Preview']} value={env} onChange={setEnv}/>
      </div>
      <div style={{display:'flex',flexDirection:'column',gap:4}}>
        {list.map((d,i)=><DeployItem key={d.id} d={d} on={d.id===D.id} last={i===list.length-1} onClick={()=>setSel(d.id)}/>)}
      </div>
    </div>
    <div style={{display:'flex',flexDirection:'column',borderRadius:'var(--radius-xl)',border:'1px solid var(--border-default)',overflow:'hidden',position:'relative'}}>
      <div style={{display:'flex',alignItems:'flex-start',gap:16,padding:'20px 20px 18px'}}>
        <StatusIcon st={D.st} size={36}/>
        <div style={{display:'flex',flexDirection:'column',gap:9,flex:1,minWidth:0}}>
          <div style={{display:'flex',alignItems:'center',gap:10}}>
            <span style={{font:'500 16px/1.2 var(--font-sans)',color:'var(--text-strong)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{D.msg}</span>
            <Badge>{D.env}</Badge>{D.current&&<Badge tone="positive">Actuel</Badge>}
          </div>
          <div style={{display:'flex',alignItems:'center',gap:12,flexWrap:'wrap',font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'}}>
            <StatusDot status={ST[D.st][0]} label={ST[D.st][1]}/>
            {mono('#'+D.id,'var(--text-muted)')}
            <span style={{display:'flex',alignItems:'center',gap:5}}><Icon name="git-commit-horizontal" size={12}/>{mono(D.sha)}</span>
            <span style={{display:'flex',alignItems:'center',gap:6}}><Avatar t={D.av}/>{D.author}</span>
            <span>{D.when}{D.dur&&' · '+D.dur}</span>
          </div>
        </div>
        <div style={{display:'flex',gap:6,flex:'none'}}>
          {D.st==='running'&&<Button icon="square" onClick={cancel}>Annuler</Button>}
          {D.st==='ok'&&!D.current&&D.env==='Production'&&<Button icon="undo-2" onClick={()=>setConfirm(D)}>Rollback</Button>}
          {D.url&&<Button icon="external-link">Visiter</Button>}
          {D.st!=='running'&&<Button variant="inverse" icon="rotate-cw" onClick={redeploy}>Redéployer</Button>}
        </div>
      </div>
      <div style={{padding:'18px 20px 20px',borderTop:hair}}><Pipeline d={D} stage={D.stage||0}/></div>
      <div style={{display:'flex',alignItems:'center',gap:10,padding:'12px 14px 12px 20px',borderTop:hair}}>
        <span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>Logs de build</span>
        <span style={lbl}>BuildKit · Railpack</span>
        {D.st==='running'&&<span style={{display:'flex',alignItems:'center',gap:6,...lbl}}><span className="live-ping" style={{width:6,height:6,borderRadius:99,background:'var(--green-500)'}}></span>en direct</span>}
        <span style={{flex:1}}></span>
        <IconButton size={28} variant="ghost" icon="download" label="Télécharger" onClick={download}/>
      </div>
      <div style={{padding:'0 14px 14px'}}><CodeBlock maxHeight={340} lines={logLines}/></div>
      {toast&&<div style={{position:'absolute',top:16,left:'50%',transform:'translateX(-50%)',padding:'8px 12px',borderRadius:'var(--radius-md)',background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)',font:'400 12px/1 var(--font-sans)',color:'var(--text-strong)',display:'flex',alignItems:'center',gap:8,whiteSpace:'nowrap'}}><Icon name="check" size={12} color="var(--green-500)"/>{toast}</div>}
    </div>
    {confirm&&<Modal title={'Revenir à #'+confirm.id+' ?'} description={'Le trafic de production sera basculé vers l\'image web:'+confirm.id+' ('+confirm.sha+') sans rebuild. Opération instantanée et réversible.'} onClose={()=>setConfirm(null)} width={480} footer={<><Button onClick={()=>setConfirm(null)}>Annuler</Button><Button variant="inverse" icon="undo-2" onClick={rollback}>Rollback</Button></>}></Modal>}
  </div>;
}
window.ProjectDeployments=ProjectDeployments;
})();
