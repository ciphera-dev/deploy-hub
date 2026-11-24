(()=>{
const {StatGroup,Stat,DataTable,StatusDot,Tabs,Button,IconButton,Badge,CodeBlock,KeyValueList,Modal,TextField,RadioCard,Switch,Icon,Select}=window.CipheraDesignSystem;
const hair='1px solid var(--border-subtle)';
const mono=(v,c)=><span style={{fontFamily:'var(--font-mono)',fontSize:12,color:c}}>{v}</span>;
const dot=v=><StatusDot status={v[0]} label={v[1]}/>;
const lbl={font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'};
const PROVIDERS={github:{icon:'github',name:'GitHub'},gitlab:{icon:'gitlab',name:'GitLab'},gitea:{icon:'git-branch',name:'Gitea'},bitbucket:{icon:'git-fork',name:'Bitbucket'},git:{icon:'link',name:'URL Git'},docker:{icon:'container',name:'Image Docker'}};
const provOf=repo=>repo.startsWith('gitlab')?'gitlab':repo.startsWith('gitea')?'gitea':'github';
const META={
  web:{domain:'app.ciphera.dev',commit:'a3f91c2',msg:'feat: nouveau tableau de bord',author:'Alex Martin',hist:'oooooooooxoooo',dur:'1 min 36 s',lang:'Next.js'},
  billing:{domain:'api.ciphera.dev',commit:'7be20d4',msg:'chore: bump sqlx 0.8',author:'Yanis Benali',hist:'oooooooooooxxx',dur:'—',lang:'Rust'},
  dash:{domain:'feat-charts.preview.ciphera.dev',commit:'c81e0a9',msg:'wip: graphiques de latence',author:'Lina Moreau',hist:'oooooxoooooop',dur:'—',lang:'Vite'},
  auth:{domain:'auth.ciphera.dev',commit:'19d4b7e',msg:'fix: rotation des clés JWT',author:'Alex Martin',hist:'oooooooooooooo',dur:'2 min 05 s',lang:'Go'},
  mkt:{domain:'ciphera.dev',commit:'5e02f3a',msg:'content: page tarifs',author:'Lina Moreau',hist:'ooooxooooooooo',dur:'48 s',lang:'Astro'},
};
const REPOS=[
  {p:'github',name:'ciphera/docs',lang:'Astro',upd:'il y a 3 h',priv:false},
  {p:'github',name:'ciphera/status-page',lang:'SvelteKit',upd:'hier',priv:false},
  {p:'github',name:'ciphera/agent',lang:'Rust',upd:'il y a 2 j',priv:true},
  {p:'gitlab',name:'ciphera/infra-scripts',lang:'Python',upd:'il y a 4 j',priv:true},
  {p:'gitlab',name:'ciphera/newsletter',lang:'Node.js',upd:'il y a 1 sem.',priv:true},
  {p:'github',name:'ciphera/cli',lang:'Rust',upd:'il y a 2 sem.',priv:false},
];
const ProvIcon=({p,size=16})=><Icon name={PROVIDERS[p].icon} size={size}/>;

function HistoryBars({hist}){
  return <div style={{display:'flex',gap:3,alignItems:'flex-end',height:18}}>{hist.split('').map((c,i)=><span key={i} title={c==='x'?'Échec':c==='p'?'En cours':'Réussi'} style={{width:4,height:c==='x'?18:c==='p'?10:14,borderRadius:1,background:c==='x'?'var(--red-500)':c==='p'?'var(--gray-500)':'var(--viz-1)'}}></span>)}</div>;
}

function ProjectCell({P,open,i}){
  const M=META[P.id]||{};const [h,setH]=React.useState(false);const prov=provOf(P.repo);
  return <div onClick={()=>open(P.id)} onMouseEnter={()=>setH(true)} onMouseLeave={()=>setH(false)} style={{padding:'22px 24px',display:'flex',flexDirection:'column',gap:16,cursor:'pointer',background:h?'var(--surface-raised)':'transparent',borderRight:i%3!==2?hair:'none',borderBottom:hair,transition:'background var(--dur-fast)'}}>
    <div style={{display:'flex',alignItems:'flex-start',gap:12}}>
      <span style={{width:32,height:32,borderRadius:8,background:'var(--surface-selected)',display:'flex',alignItems:'center',justifyContent:'center',color:'var(--text-body)',flex:'none'}}><ProvIcon p={prov}/></span>
      <div style={{display:'flex',flexDirection:'column',gap:6,minWidth:0,flex:1}}>
        <span style={{font:'500 14px/1.1 var(--font-sans)',color:'var(--text-strong)'}}>{P.name}</span>
        <span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{M.domain}</span>
      </div>
      <Badge>{P.env}</Badge>
    </div>
    <div style={{display:'flex',flexDirection:'column',gap:8}}>
      <span style={{font:'400 13px/1.3 var(--font-sans)',color:'var(--text-body)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{M.msg}</span>
      <div style={{display:'flex',alignItems:'center',gap:10,font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'}}>
        <span style={{display:'flex',alignItems:'center',gap:5}}><Icon name="git-branch" size={12}/>{mono(P.branch)}</span>
        <span style={{display:'flex',alignItems:'center',gap:5}}><Icon name="git-commit-horizontal" size={12}/>{mono(M.commit)}</span>
        <span style={{flex:1}}></span><span>{P.updated}</span>
      </div>
    </div>
    <div style={{display:'flex',alignItems:'center',justifyContent:'space-between'}}>{dot(P.status)}<HistoryBars hist={M.hist||''}/></div>
  </div>;
}

function ImportStrip({onImport}){
  return <div style={{display:'grid',gridTemplateColumns:'minmax(0,1fr) minmax(0,1.6fr)',borderBottom:hair}}>
    <div style={{padding:'28px 32px 28px 46px',borderRight:hair,display:'flex',flexDirection:'column',gap:18}}>
      <div style={{display:'flex',flexDirection:'column',gap:8}}><span style={lbl}>Fournisseurs Git</span><span style={{font:'500 16px/1.2 var(--font-sans)',color:'var(--text-strong)'}}>Déployez depuis n'importe quel dépôt</span></div>
      <div style={{display:'flex',flexDirection:'column',gap:2}}>
        {[['github','ciphera · 34 dépôts',true],['gitlab','ciphera-group · 8 dépôts',true],['gitea','Non connecté',false],['bitbucket','Non connecté',false]].map(([p,sub,on])=><div key={p} style={{display:'flex',alignItems:'center',gap:12,padding:'8px 0'}}>
          <span style={{color:on?'var(--text-strong)':'var(--text-subtle)'}}><ProvIcon p={p}/></span>
          <span style={{font:'500 13px/1 var(--font-sans)',color:on?'var(--text-strong)':'var(--text-muted)',width:78}}>{PROVIDERS[p].name}</span>
          <span style={{...lbl,flex:1}}>{sub}</span>
          {on?<StatusDot status="ok" label="Connecté"/>:<Button size="sm">Connecter</Button>}
        </div>)}
      </div>
    </div>
    <div style={{padding:'28px 32px 28px 32px',display:'flex',flexDirection:'column',gap:14}}>
      <div style={{display:'flex',alignItems:'center',justifyContent:'space-between'}}><span style={lbl}>Importer un dépôt récent</span><Button variant="ghost" icon="search" onClick={()=>onImport()}>Tous les dépôts</Button></div>
      <div style={{display:'flex',flexDirection:'column'}}>
        {REPOS.slice(0,4).map(r=><div key={r.name} style={{display:'flex',alignItems:'center',gap:12,padding:'10px 0',borderTop:hair}}>
          <span style={{color:'var(--text-muted)'}}><ProvIcon p={r.p} size={15}/></span>
          <span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{r.name}</span>
          {r.priv&&<Icon name="lock" size={11} color="var(--text-faint)"/>}
          <span style={lbl}>{r.lang} · {r.upd}</span><span style={{flex:1}}></span>
          <Button size="sm" variant="filled" onClick={()=>onImport(r)}>Importer</Button>
        </div>)}
      </div>
    </div>
  </div>;
}

function ProjectsList({open,onImport}){
  const D=window.CIPHERA;
  const [env,setEnv]=React.useState('Tous');const [q,setQ]=React.useState('');const [view,setView]=React.useState('grid');
  const list=D.projects.filter(p=>(env==='Tous'||p.env===env)&&(p.name+p.repo).toLowerCase().includes(q.toLowerCase()));
  const imp=onImport||(()=>{});
  return <div>
    <div style={{padding:'16px 32px 28px 46px',borderBottom:hair}}><StatGroup><Stat label="Projets actifs" value="12"/><Stat label="Déploiements (30 j)" value="1 486" delta="12.4%" deltaSuffix="vs période précédente"/><Stat label="Durée moyenne de build" value="42 s" delta="3.1%" deltaSuffix="plus rapide"/><Stat label="Taux de succès" value="97.3%" delta="0.6%" deltaSuffix="vs période précédente"/></StatGroup></div>
    <ImportStrip onImport={imp}/>
    <div style={{display:'flex',alignItems:'center',gap:12,padding:'22px 32px 18px 46px'}}>
      <Tabs tabs={['Tous','Production','Preview','Staging']} value={env} onChange={setEnv}/>
      <span style={{flex:1}}></span>
      <div style={{display:'flex',alignItems:'center',gap:8,height:32,padding:'0 10px',width:240,borderRadius:'var(--radius-md)',border:'1px solid var(--border-strong)',background:'var(--surface-control)'}}>
        <Icon name="search" size={14} color="var(--text-subtle)"/>
        <input value={q} onChange={e=>setQ(e.target.value)} placeholder="Rechercher un projet…" style={{flex:1,minWidth:0,background:'transparent',border:0,outline:'none',color:'var(--text-strong)',font:'400 13px/1 var(--font-sans)'}}/>
      </div>
      <div style={{display:'flex',gap:4}}><IconButton icon="layout-grid" variant={view==='grid'?'secondary':'ghost'} onClick={()=>setView('grid')} label="Grille"/><IconButton icon="list" variant={view==='list'?'secondary':'ghost'} onClick={()=>setView('list')} label="Liste"/></div>
    </div>
    {view==='grid'?<div style={{display:'grid',gridTemplateColumns:'repeat(3,minmax(0,1fr))',borderTop:hair,margin:'0 0 40px 0'}}>
      {list.map((P,i)=><ProjectCell key={P.id} P={P} open={open} i={i}/>)}
      <div onClick={()=>imp()} style={{padding:'22px 24px',display:'flex',flexDirection:'column',alignItems:'center',justifyContent:'center',gap:10,minHeight:180,cursor:'pointer',color:'var(--text-subtle)',borderBottom:hair,borderRight:list.length%3!==2?hair:'none'}}>
        <span style={{width:32,height:32,borderRadius:99,border:'1px dashed var(--gray-500)',display:'flex',alignItems:'center',justifyContent:'center'}}><Icon name="plus" size={14}/></span>
        <span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-body)'}}>Nouveau projet</span>
        <span style={lbl}>GitHub, GitLab, Gitea, Bitbucket ou image Docker</span>
      </div>
    </div>:<div style={{padding:'0 32px 40px 34px'}}>
      <DataTable onRowClick={r=>open(r.id)} columns={[{key:'name',label:'Projet',width:'1.3fr'},{key:'repo',label:'Dépôt',width:'1.5fr',render:(v)=><span style={{display:'flex',alignItems:'center',gap:6}}><ProvIcon p={provOf(v)} size={13}/>{mono(v.split('/').slice(1).join('/'))}</span>},{key:'env',label:'Environnement'},{key:'status',label:'Statut',render:dot},{key:'branch',label:'Branche',render:v=>mono(v)},{key:'updated',label:'Mis à jour',align:'right'}]} rows={list}/>
    </div>}
  </div>;
}

function ProjectDetail({id,back}){
  const D=window.CIPHERA,P=D.projects.find(p=>p.id===id)||D.projects[0],S=D.services[P.id]||[],M=META[P.id]||META.web,prov=provOf(P.repo);
  const [tab,setTab]=React.useState('Aperçu');
  const [log,setLog]=React.useState(D.deployments[0].id);
  const [auto,setAuto]=React.useState(true);const [prev,setPrev]=React.useState(true);const [skip,setSkip]=React.useState(false);
  return <div>
    <div style={{padding:'20px 32px 0 46px',display:'flex',flexDirection:'column',gap:20}}>
      <button onClick={back} style={{alignSelf:'flex-start',display:'flex',alignItems:'center',gap:6,background:'none',border:0,padding:0,color:'var(--text-subtle)',font:'400 12px/1 var(--font-sans)',cursor:'pointer'}}><Icon name="arrow-left" size={13}/>Projets</button>
      <div style={{display:'flex',alignItems:'flex-end',justifyContent:'space-between',gap:24}}>
        <div style={{display:'flex',flexDirection:'column',gap:10}}>
          <div style={{display:'flex',alignItems:'center',gap:12}}><span style={{font:'500 22px/1.1 var(--font-sans)',color:'var(--text-strong)'}}>{P.name}</span><Badge>{P.env}</Badge></div>
          <div style={{display:'flex',gap:18,alignItems:'center',font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'}}>{dot(P.status)}<span style={{display:'flex',gap:6,alignItems:'center'}}><ProvIcon p={prov} size={13}/>{mono(P.repo)}</span><span style={{display:'flex',gap:6,alignItems:'center'}}><Icon name="server" size={13}/>{P.server}</span></div>
        </div>
        <div style={{display:'flex',gap:8}}><Button icon="external-link">Visiter</Button><Button icon="rotate-ccw">Redémarrer</Button><Button variant="inverse" icon="rocket">Redéployer</Button></div>
      </div>
      <Tabs tabs={['Aperçu',{value:'Services',label:'Services',count:S.length},'Déploiements','Logs','Variables','Domaines','Git']} value={tab} onChange={setTab}/>
    </div>
    {tab==='Aperçu'&&<window.ProjectOverview P={P} M={M} S={S} deps={D.deployments} go={setTab}/>}
    {tab==='Services'&&<window.ProjectServices S={S} go={setTab}/>}
    <div style={{padding:tab==='Aperçu'||tab==='Services'?0:'24px 32px 40px 34px'}}>
      {tab==='Déploiements'&&<window.ProjectDeployments project={P.name}/>}
      {tab==='Logs'&&<window.ProjectLogs project={P.name}/>}
      {tab==='Variables'&&<window.ProjectVariables project={P.name}/>}
      {tab==='Domaines'&&<window.ProjectDomains/>}
      {tab==='Git'&&<window.ProjectGit project={P.name} repo={P.repo} branch={P.branch}/>}
    </div>
  </div>;
}

const STEPS=['Source','Configuration','Déploiement'];
const BUILD_LINES=[
  ['Clonage du dépôt (profondeur 1)…','info'],['Commit a3f91c2 · feat: nouveau tableau de bord','muted'],['Railpack : Node.js 22 détecté (pnpm)','info'],
  ['BuildKit : résolution du cache distant','muted'],['#4 pnpm install --frozen-lockfile','info'],['#4 DONE 6.8s','ok'],['#5 pnpm build','info'],['#5 ✓ Compiled successfully','ok'],
  ['Export de l\'image OCI (148 Mo)','info'],['Push vers le registre interne','muted'],['Agent srv-par-01 : démarrage du conteneur','info'],['Healthcheck GET /health → 200','ok'],['Certificat TLS émis par Caddy','ok'],['Trafic basculé, déploiement en ligne','ok']];

function Stepper({step}){
  return <div style={{display:'flex',alignItems:'center',gap:10}}>{STEPS.map((s,i)=><React.Fragment key={s}>
    {i>0&&<span style={{width:24,height:1,background:'var(--border-strong)'}}></span>}
    <span style={{display:'flex',alignItems:'center',gap:8,font:'500 12px/1 var(--font-sans)',color:i<=step?'var(--text-strong)':'var(--text-faint)'}}>
      <span style={{width:18,height:18,borderRadius:99,display:'flex',alignItems:'center',justifyContent:'center',background:i<step?'var(--surface-inverse)':i===step?'var(--surface-selected)':'transparent',border:i>step?'1px solid var(--border-strong)':'none',color:i<step?'var(--text-inverse)':'inherit',fontSize:10}}>{i<step?<Icon name="check" size={11}/>:i+1}</span>{s}</span>
  </React.Fragment>)}</div>;
}

function DeployAnim({pct,done}){
  const ST=[['Clonage','2 s'],['Build','41 s'],['Image','9 s'],['En ligne','3 s']];
  const cur=done?4:Math.min(3,Math.floor(pct/25));
  return <div style={{flex:'none',display:'flex',flexDirection:'column',gap:16,padding:'18px 20px',borderRadius:14,border:'1px solid rgba(255,255,255,.08)',background:'rgba(255,255,255,.02)'}}>
    <div style={{position:'relative',display:'grid',gridTemplateColumns:'repeat(4,1fr)'}}>
      <span style={{position:'absolute',left:'12.5%',right:'12.5%',top:11,height:2,borderRadius:2,background:'rgba(255,255,255,.08)'}}></span>
      <span style={{position:'absolute',left:'12.5%',top:11,height:2,borderRadius:2,width:(Math.min(100,done?100:pct)*.75)+'%',background:done?'#86efac':'#a78bfa',transition:'width .4s ease-out, background .4s'}}></span>
      {ST.map(([n,tm],i)=>{const ok=i<cur,act=i===cur&&!done;return <div key={n} style={{position:'relative',display:'flex',flexDirection:'column',alignItems:'center',gap:8}}>
        <span style={{position:'relative',width:24,height:24,borderRadius:99,display:'flex',alignItems:'center',justifyContent:'center',background:ok?(done?'#86efac':'#a78bfa'):'#0c0c0f',border:'2px solid '+(ok?(done?'#86efac':'#a78bfa'):act?'#a78bfa':'rgba(255,255,255,.14)'),transition:'all .3s'}}>
          {act&&<span style={{position:'absolute',inset:-6,borderRadius:99,border:'1px solid rgba(167,139,250,.6)',animation:'depPulse 1.4s ease-out infinite'}}></span>}
          {ok?<Icon name="check" size={12} color="#0b0b0b"/>:act?<span style={{width:8,height:8,borderRadius:99,background:'#a78bfa'}}></span>:null}
        </span>
        <span style={{display:'flex',flexDirection:'column',alignItems:'center',gap:4}}><span style={{font:'500 12px/1 var(--font-sans)',color:ok||act?'var(--text-strong)':'var(--text-subtle)'}}>{n}</span><span style={{font:'400 11px/1 var(--font-mono)',color:'var(--text-faint)'}}>{ok?tm:act?'…':'—'}</span></span>
      </div>})}
    </div>
    <div style={{display:'flex',alignItems:'center',gap:10,paddingTop:14,borderTop:'1px solid rgba(255,255,255,.06)'}}>
      {done?<><span style={{width:22,height:22,borderRadius:99,display:'flex',alignItems:'center',justifyContent:'center',background:'rgba(134,239,172,.15)'}}><Icon name="check" size={12} color="#86efac"/></span><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>Déployé avec succès</span><span style={{flex:1}}></span><span style={{font:'400 12px/1 var(--font-mono)',color:'var(--text-muted)'}}>55 s</span></>
      :<><Icon name="loader" size={14} color="#a78bfa" style={{animation:'spin 1s linear infinite'}}/><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{ST[cur][0]} en cours</span><span style={{flex:1}}></span><span style={{font:'500 12px/1 var(--font-mono)',color:'var(--text-muted)'}}>{Math.round(pct)} %</span></>}
    </div>
  </div>;
}
function NewProjectModal({onClose,onCreate,initial}){
  const [step,setStep]=React.useState(initial?1:0);
  const [prov,setProv]=React.useState(initial?.p||'github');
  const [repo,setRepo]=React.useState(initial||null);
  const [q,setQ]=React.useState('');
  const [name,setName]=React.useState(initial?initial.name.split('/')[1]:'');
  const [b,setB]=React.useState('railpack');
  const [branch,setBranch]=React.useState('main');
  const [server,setServer]=React.useState('srv-par-01 · Paris');
  const [env,setEnv]=React.useState([['NODE_ENV','production'],['PORT','3000']]);
  const [auto,setAuto]=React.useState(true);const [prev,setPrev]=React.useState(true);
  const [lines,setLines]=React.useState(0);
  React.useEffect(()=>{if(step!==2)return;setLines(0);const t=setInterval(()=>setLines(n=>{if(n>=BUILD_LINES.length){clearInterval(t);return n}return n+1}),380);return()=>clearInterval(t)},[step]);
  const done=step===2&&lines>=BUILD_LINES.length;
  const pick=r=>{setRepo(r);setName(r.name.split('/')[1]);setStep(1)};
  const repos=REPOS.filter(r=>r.p===prov&&r.name.includes(q));
  const isGit=['github','gitlab','gitea','bitbucket'].includes(prov);
  const footer=step===0?<><Button onClick={onClose}>Annuler</Button></>
    :step===1?<><Button onClick={()=>setStep(0)} icon="arrow-left">Retour</Button><Button variant="inverse" icon="rocket" onClick={()=>setStep(2)}>Déployer</Button></>
    :<><Button onClick={onClose}>{done?'Fermer':'Continuer en arrière-plan'}</Button><Button variant="inverse" disabled={!done} iconRight="arrow-right" onClick={()=>onCreate(name)}>Ouvrir le projet</Button></>;
  return <Modal title="Nouveau projet" description={<Stepper step={step}/>} onClose={onClose} width={720} footer={footer}>
    <div style={{maxHeight:'62vh',overflowY:'auto',display:'flex',flexDirection:'column',gap:20,margin:'0 -2px',padding:'0 2px'}} className="ds-scroll-hidden">
    {step===0&&<>
      <div style={{display:'grid',gridTemplateColumns:'repeat(6,minmax(0,1fr))',gap:6}}>
        {Object.entries(PROVIDERS).map(([k,v])=><button key={k} onClick={()=>setProv(k)} style={{display:'flex',flexDirection:'column',alignItems:'center',gap:8,padding:'14px 6px',borderRadius:'var(--radius-lg)',cursor:'pointer',background:prov===k?'var(--surface-selected)':'var(--surface-control)',border:'1px solid '+(prov===k?'var(--gray-500)':'var(--border-strong)'),color:prov===k?'var(--text-strong)':'var(--text-muted)',font:'500 12px/1 var(--font-sans)'}}><Icon name={v.icon} size={18}/>{v.name}</button>)}
      </div>
      {isGit?<>
        <div style={{display:'flex',gap:8}}>
          <Select options={prov==='gitlab'?['ciphera-group']:['ciphera','alex-martin']}/>
          <div style={{display:'flex',alignItems:'center',gap:8,height:32,padding:'0 10px',flex:1,borderRadius:'var(--radius-md)',border:'1px solid var(--border-strong)',background:'var(--surface-control)'}}>
            <Icon name="search" size={14} color="var(--text-subtle)"/><input autoFocus value={q} onChange={e=>setQ(e.target.value)} placeholder="Rechercher un dépôt…" style={{flex:1,background:'transparent',border:0,outline:'none',color:'var(--text-strong)',font:'400 13px/1 var(--font-sans)'}}/>
          </div>
        </div>
        {(prov==='gitea'||prov==='bitbucket')?<div style={{display:'flex',flexDirection:'column',alignItems:'center',gap:12,padding:'32px 0',textAlign:'center'}}>
          <span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{PROVIDERS[prov].name} n'est pas connecté</span>
          <span style={lbl}>Autorisez Ciphera à lire vos dépôts et à créer des webhooks.</span>
          <Button variant="filled" icon={PROVIDERS[prov].icon}>Connecter {PROVIDERS[prov].name}</Button>
        </div>:<div style={{display:'flex',flexDirection:'column',border:'1px solid var(--border-default)',borderRadius:'var(--radius-lg)'}}>
          {repos.map((r,i)=><div key={r.name} style={{display:'flex',alignItems:'center',gap:12,padding:'12px 14px',borderTop:i?hair:'none'}}>
            <ProvIcon p={r.p} size={15}/><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{r.name}</span>{r.priv&&<Icon name="lock" size={11} color="var(--text-faint)"/>}
            <span style={lbl}>{r.lang} · {r.upd}</span><span style={{flex:1}}></span><Button size="sm" variant="filled" onClick={()=>pick(r)}>Importer</Button>
          </div>)}
          {!repos.length&&<div style={{padding:'20px 14px',...lbl}}>Aucun dépôt trouvé.</div>}
        </div>}
      </>:<div style={{display:'flex',flexDirection:'column',gap:16}}>
        <TextField label={prov==='docker'?'Image':'URL du dépôt'} mono placeholder={prov==='docker'?'ghcr.io/org/app:latest':'https://git.exemple.com/org/app.git'} hint={prov==='docker'?'Registre public ou privé (identifiants dans Paramètres › Secrets).':'Dépôt public, ou privé via une clé de déploiement SSH.'}/>
        <div><Button variant="filled" iconRight="arrow-right" onClick={()=>pick({p:prov,name:prov==='docker'?'org/app':'org/app',lang:'—'})}>Continuer</Button></div>
      </div>}
    </>}
    {step===1&&<>
      <div style={{display:'flex',alignItems:'center',gap:12,padding:'12px 14px',borderRadius:'var(--radius-lg)',border:'1px solid var(--border-default)'}}>
        <Icon name={PROVIDERS[prov].icon} size={16}/><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{repo?.name}</span><span style={{flex:1}}></span>
        <span style={{display:'flex',alignItems:'center',gap:6,...lbl}}><Icon name="git-branch" size={12}/></span><Select options={['main','develop','feat/charts']} value={branch} onChange={setBranch}/>
      </div>
      <div style={{display:'flex',gap:16}}><TextField label="Nom du projet" value={name} onChange={setName} style={{flex:1}}/><TextField label="Dossier racine" mono defaultValue="./" style={{flex:1}}/></div>
      <div style={{display:'flex',flexDirection:'column',gap:8}}>
        <span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>Méthode de build</span>
        <div style={{display:'grid',gridTemplateColumns:'repeat(3,minmax(0,1fr))',gap:8}}>
          <RadioCard icon="package" title="Railpack" description="Détection auto" selected={b==='railpack'} onClick={()=>setB('railpack')}/>
          <RadioCard icon="file-code" title="Dockerfile" description="./Dockerfile" selected={b==='docker'} onClick={()=>setB('docker')}/>
          <RadioCard icon="boxes" title="Compose" description="compose.yaml" selected={b==='compose'} onClick={()=>setB('compose')}/>
        </div>
      </div>
      {b==='railpack'&&<div style={{display:'flex',gap:16}}><TextField label="Commande de build" mono placeholder="pnpm build" style={{flex:1}}/><TextField label="Commande de démarrage" mono placeholder="pnpm start" style={{flex:1}}/><TextField label="Port" mono defaultValue="3000" style={{width:90}}/></div>}
      <div style={{display:'flex',flexDirection:'column',gap:8}}>
        <span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>Variables d'environnement</span>
        {env.map(([k,v],i)=><div key={i} style={{display:'flex',gap:8,alignItems:'center'}}>
          <input value={k} onChange={e=>setEnv(x=>x.map((r,j)=>j===i?[e.target.value,r[1]]:r))} placeholder="CLÉ" style={{flex:1,height:32,padding:'0 10px',borderRadius:'var(--radius-md)',border:'1px solid var(--border-strong)',background:'var(--surface-control)',color:'var(--text-strong)',font:'400 12px/1 var(--font-mono)',outline:'none'}}/>
          <input value={v} onChange={e=>setEnv(x=>x.map((r,j)=>j===i?[r[0],e.target.value]:r))} placeholder="valeur" style={{flex:1.4,height:32,padding:'0 10px',borderRadius:'var(--radius-md)',border:'1px solid var(--border-strong)',background:'var(--surface-control)',color:'var(--text-strong)',font:'400 12px/1 var(--font-mono)',outline:'none'}}/>
          <IconButton icon="x" variant="ghost" onClick={()=>setEnv(x=>x.filter((_,j)=>j!==i))} label="Supprimer"/>
        </div>)}
        <div style={{display:'flex',gap:8}}><Button size="sm" icon="plus" onClick={()=>setEnv(x=>[...x,['','']])}>Ajouter</Button><Button size="sm" variant="ghost" icon="clipboard">Coller un .env</Button></div>
      </div>
      <div style={{display:'flex',flexDirection:'column',gap:8}}><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>Serveur cible</span><Select options={['srv-par-01 · Paris','srv-par-02 · Paris','srv-fra-01 · Francfort']} value={server} onChange={setServer}/></div>
      <Switch checked={auto} onChange={setAuto} label="Déploiement automatique" description={'Redéploie à chaque push sur '+branch+'.'}/>
      <Switch checked={prev} onChange={setPrev} label="Previews de pull request" description="Un environnement éphémère par PR."/>
    </>}
    {step===2&&<>
      <div style={{display:'flex',alignItems:'center',justifyContent:'space-between'}}>
        <div style={{display:'flex',flexDirection:'column',gap:8}}>
          <span style={{font:'500 16px/1.2 var(--font-sans)',color:'var(--text-strong)'}}>{done?'Projet en ligne':'Build en cours…'}</span>
          <span style={lbl}>{repo?.name} · {branch} · {server}</span>
        </div>
        {done?<StatusDot status="ok" label={name+'.ciphera.app'}/>:<StatusDot status="pending" label={Math.round(lines/BUILD_LINES.length*100)+' %'}/>}
      </div>
      <DeployAnim pct={lines/BUILD_LINES.length*100} done={done}/>
      <CodeBlock maxHeight={300} lines={BUILD_LINES.slice(0,lines).map(([t,tone],i)=>({time:'12:04:'+String(10+i*3).padStart(2,'0'),text:t,tone}))}/>
    </>}
    </div>
  </Modal>;
}
Object.assign(window,{ProjectsList,ProjectDetail,NewProjectModal});
})();
