(()=>{
const {Button,IconButton,Icon,Modal,Switch}=window.CipheraDesignSystem;
const lbl={font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'};
const line='1px solid rgba(255,255,255,.06)';
const box={borderRadius:'var(--radius-xl)',border:'1px solid var(--border-default)',background:'var(--surface-app)'};
const mono=(v,c='var(--text-body)',s=12)=><span style={{font:'400 '+s+'px/1 var(--font-mono)',color:c,whiteSpace:'nowrap'}}>{v}</span>;
function useToast(){const [t,s]=React.useState('');const r=React.useRef();return [t,m=>{s(m);clearTimeout(r.current);r.current=setTimeout(()=>s(''),2200)}]}
function Toast({t}){return t?<div style={{position:'fixed',bottom:24,right:24,zIndex:80,padding:'10px 14px',borderRadius:'var(--radius-md)',background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)',font:'400 12px/1 var(--font-sans)',color:'var(--text-strong)',display:'flex',alignItems:'center',gap:8}}><Icon name="check" size={12} color="var(--green-500)"/>{t}</div>:null}
function Banner({icon,title,sub,right}){return <div style={{borderRadius:20,border:'1px solid var(--border-default)',background:'var(--surface-app)',padding:'22px 24px',display:'flex',alignItems:'center',gap:16}}>
  <span style={{width:52,height:52,borderRadius:13,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'linear-gradient(145deg,#4a4a52,#26262c)',boxShadow:'inset 0 1px 0 rgba(255,255,255,.18)'}}><Icon name={icon} size={24} color="#f2f2f2"/></span>
  <span style={{display:'flex',flexDirection:'column',gap:8,flex:1,minWidth:0}}><span style={{font:'600 18px/1.1 var(--font-sans)',color:'var(--text-strong)'}}>{title}</span><span style={{font:'400 13px/1.45 var(--font-sans)',color:'var(--text-muted)'}}>{sub}</span></span>
  <span style={{display:'flex',gap:8,alignItems:'center'}}>{right}</span></div>}
const wbtn=(ic,t,f,solid)=><button onClick={f} style={{display:'flex',alignItems:'center',gap:8,height:38,padding:'0 15px',borderRadius:10,cursor:'pointer',border:solid?0:'1px solid var(--border-strong)',background:solid?'#8b7cf6':'transparent',color:solid?'#fff':'var(--text-strong)',font:'600 13px/1 var(--font-sans)'}}><Icon name={ic} size={14} color={solid?'#fff':'var(--text-strong)'}/>{t}</button>;

/* ---------- Webhooks ---------- */
const EVENTS=['deploy.succeeded','deploy.failed','server.offline','domain.ssl_expiring','security.ban','ticket.created'];
const HOOKS=[
  {id:1,url:'https://hooks.slack.com/services/T04/B07/xyz',name:'Slack #deploys',events:['deploy.succeeded','deploy.failed'],on:true,rate:99.8,hist:[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1],last:'il y a 2 min',lat:'84 ms'},
  {id:2,url:'https://api.pagerduty.com/v2/enqueue',name:'PagerDuty astreinte',events:['server.offline','deploy.failed','security.ban'],on:true,rate:96.1,hist:[1,1,1,0,1,1,1,1,1,1,1,1,0,1,1,1,1,1,1,1],last:'il y a 3 h',lat:'212 ms'},
  {id:3,url:'https://ci.example.com/hooks/ciphera',name:'CI interne',events:['deploy.succeeded'],on:false,rate:72.4,hist:[1,0,0,1,0,1,1,0,1,1,0,1,1,1,0,1,1,0,1,1],last:'il y a 2 j',lat:'1,4 s'},
];
const DELIV=[[1,'deploy.succeeded',200,'84 ms','il y a 2 min'],[2,'deploy.failed',200,'230 ms','il y a 3 h'],[2,'server.offline',503,'—','il y a 3 h'],[1,'deploy.failed',200,'91 ms','hier'],[3,'deploy.succeeded',404,'1,4 s','il y a 2 j']];
function Fav({url,size=18}){
  const [err,setErr]=React.useState(false);let host='';try{host=new URL(url).hostname}catch(e){}
  React.useEffect(()=>setErr(false),[url]);
  if(!host||err)return <Icon name="send" size={15} color="var(--text-strong)"/>;
  return <img src={'https://www.google.com/s2/favicons?domain='+host+'&sz=64'} alt="" onError={()=>setErr(true)} style={{width:size,height:size,objectFit:'contain',borderRadius:4,display:'block'}}/>;
}
function HookModal({onClose,onSave}){
  const [url,setUrl]=React.useState('');const [name,setName]=React.useState('');const [ev,setEv]=React.useState(['deploy.failed']);
  const ok=/^https:\/\/\S+\.\S+/.test(url)&&ev.length;
  const field=(v,s,ph,m)=><input value={v} onChange={e=>s(e.target.value)} placeholder={ph} style={{height:40,padding:'0 12px',borderRadius:10,border:'1px solid rgba(255,255,255,.1)',background:'rgba(255,255,255,.045)',color:'var(--text-strong)',font:'400 13px/1 '+(m?'var(--font-mono)':'var(--font-sans)'),outline:'none'}}/>;
  return <Modal title="Nouveau webhook" description="Ciphera envoie un POST JSON signé (HMAC SHA-256) à chaque événement choisi." onClose={onClose} width={560} footer={<><Button onClick={onClose}>Annuler</Button><Button variant="inverse" icon="plus" disabled={!ok} onClick={()=>onSave({url,name:name||new URL(url).hostname,events:ev,favicon:'https://www.google.com/s2/favicons?domain='+new URL(url).hostname+'&sz=64'})}>Créer le webhook</Button></>}>
    <div style={{display:'flex',flexDirection:'column',gap:14}}>
      <div style={{display:'flex',flexDirection:'column',gap:8}}><span style={lbl}>URL de destination</span><div style={{display:'flex',alignItems:'center',gap:10}}><span style={{width:40,height:40,borderRadius:10,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-selected)',border:'1px solid var(--border-subtle)'}}><Fav url={url}/></span><div style={{flex:1,display:'flex',flexDirection:'column'}}>{field(url,setUrl,'https://exemple.com/webhooks/ciphera',true)}</div></div><span style={{...lbl,fontSize:11}}>L'icône du service est récupérée automatiquement depuis l'adresse.</span></div>
      <div style={{display:'flex',flexDirection:'column',gap:8}}><span style={lbl}>Nom (facultatif)</span>{field(name,setName,'Slack #deploys')}</div>
      <div style={{display:'flex',flexDirection:'column',gap:8}}><span style={lbl}>Événements</span><div style={{display:'flex',gap:6,flexWrap:'wrap'}}>{EVENTS.map(e=>{const on=ev.includes(e);return <button key={e} onClick={()=>setEv(x=>on?x.filter(y=>y!==e):[...x,e])} style={{display:'flex',alignItems:'center',gap:6,height:30,padding:'0 11px',borderRadius:99,cursor:'pointer',font:'400 12px/1 var(--font-mono)',border:'1px solid '+(on?'rgba(167,139,250,.45)':'rgba(255,255,255,.08)'),background:on?'rgba(167,139,250,.12)':'transparent',color:on?'var(--text-strong)':'var(--text-muted)'}}><Icon name={on?'check':'plus'} size={11} color={on?'#c4b5fd':'var(--text-subtle)'}/>{e}</button>})}</div></div>
    </div>
  </Modal>;
}
function WebhooksPage(){
  const [hooks,setHooks]=React.useState(HOOKS);const [add,setAdd]=React.useState(false);const [t,flash]=useToast();
  const COL='167,139,250';
  return <div style={{padding:'20px 32px 40px 34px',display:'flex',flexDirection:'column',gap:16}}>
    <Banner col={COL} grad="linear-gradient(110deg,#5b4bd6,#8b7cf6 60%,#a99cff)" icon="webhook" title="Envoyez les événements de Ciphera vers vos outils" sub={hooks.filter(h=>h.on).length+' actifs · '+EVENTS.length+' types d\'événements · signature HMAC SHA-256 · 3 nouvelles tentatives en cas d\'échec'} right={<>{wbtn('book-open','Format du payload',()=>flash('Documentation des webhooks'))}{wbtn('plus','Nouveau webhook',()=>setAdd(true),true)}</>}/>
    <div style={{display:'flex',flexDirection:'column',gap:12}}>
      {hooks.map(h=><div key={h.id} style={{...box,overflow:'hidden',opacity:h.on?1:.65}}>
        <div style={{display:'flex',alignItems:'center',gap:14,padding:'16px 18px'}}>
          <span style={{width:36,height:36,borderRadius:10,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'linear-gradient(180deg,#1f1f24,#16161a)',border:'1px solid rgba(255,255,255,.1)'}}><Fav url={h.url}/></span>
          <span style={{display:'flex',flexDirection:'column',gap:7,flex:1,minWidth:0}}><span style={{font:'600 14px/1 var(--font-sans)',color:'var(--text-strong)'}}>{h.name}</span><span style={{font:'400 12px/1 var(--font-mono)',color:'var(--text-muted)',overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'}}>{h.url}</span></span>
          <span style={{display:'flex',flexDirection:'column',gap:6,alignItems:'flex-end'}}><span style={{display:'flex',gap:2}}>{h.hist.map((v,i)=><span key={i} style={{width:4,height:16,borderRadius:2,background:v?'#86efac':'#f87171',opacity:v?.75:1}}></span>)}</span><span style={{...lbl,fontSize:11}}>{h.rate.toLocaleString('fr-FR')} % de succès · {h.lat}</span></span>
          <Switch checked={h.on} onChange={v=>{setHooks(x=>x.map(y=>y.id===h.id?{...y,on:v}:y));flash(h.name+(v?' activé':' désactivé'))}}/>
        </div>
        <div style={{display:'flex',alignItems:'center',gap:8,flexWrap:'wrap',padding:'11px 18px',borderTop:line,background:'rgba(255,255,255,.015)'}}>
          {h.events.map(e=><span key={e} style={{height:22,display:'flex',alignItems:'center',padding:'0 8px',borderRadius:6,background:'rgba(255,255,255,.05)',font:'400 11px/1 var(--font-mono)',color:'var(--text-body)'}}>{e}</span>)}
          <span style={{flex:1}}></span><span style={{...lbl,fontSize:11}}>Dernier envoi {h.last}</span>
          <Button size="sm" variant="ghost" icon="send" onClick={()=>flash('Événement de test envoyé à '+h.name)}>Tester</Button>
          <IconButton size={28} variant="ghost" icon="trash-2" label="Supprimer" onClick={()=>{setHooks(x=>x.filter(y=>y.id!==h.id));flash(h.name+' supprimé')}}/>
        </div>
      </div>)}
    </div>
    <div style={{...box,overflow:'hidden'}}>
      <div style={{display:'flex',alignItems:'center',padding:'14px 18px',borderBottom:line}}><span style={{font:'600 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>Livraisons récentes</span></div>
      {DELIV.map(([hid,ev,code,lat,w],i)=>{const h=hooks.find(x=>x.id===hid)||HOOKS.find(x=>x.id===hid);const ok=code<300;return <div key={i} style={{display:'grid',gridTemplateColumns:'60px minmax(0,1fr) minmax(0,1fr) 80px 100px 32px',gap:14,alignItems:'center',minHeight:48,padding:'0 12px 0 18px',borderTop:i?line:'none'}}>
        <span style={{width:'fit-content',height:22,display:'flex',alignItems:'center',padding:'0 7px',borderRadius:6,font:'600 11px/1 var(--font-mono)',background:ok?'rgba(134,239,172,.12)':'rgba(248,113,113,.12)',color:ok?'#86efac':'#f87171'}}>{code}</span>
        {mono(ev,'var(--text-strong)')}<span style={{...lbl,overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'}}>{h.name}</span>{mono(lat,'var(--text-muted)',11)}<span style={lbl}>{w}</span>
        <IconButton size={28} variant="ghost" icon="rotate-cw" label="Renvoyer" onClick={()=>flash(ev+' renvoyé')}/>
      </div>})}
    </div>
    {add&&<HookModal onClose={()=>setAdd(false)} onSave={h=>{setHooks(x=>[...x,{...h,id:Date.now(),on:true,rate:100,hist:Array(20).fill(1),last:'jamais',lat:'—'}]);setAdd(false);flash('Webhook créé')}}/>}
    <Toast t={t}/>
  </div>;
}

/* ---------- Raccourcis clavier ---------- */
const SC=[
  ['Général','command','125,211,252',[['Recherche globale',['⌘','K']],['Afficher les raccourcis',['⌘','/']],['Replier la barre latérale',['⌘','B']],['Fermer une fenêtre',['Échap']]]],
  ['Navigation','compass','167,139,250',[['Vue d\'ensemble',['G','O']],['Projets',['G','P']],['Serveurs',['G','S']],['Déploiements',['G','D']],['Tickets',['G','T']]]],
  ['Actions','zap','134,239,172',[['Nouveau projet',['N','P']],['Nouveau ticket',['N','T']],['Redéployer le projet ouvert',['⇧','R']],['Copier l\'URL du projet',['⌘','⇧','C']]]],
  ['Logs et terminal','terminal','251,191,36',[['Mettre en pause le flux',['Espace']],['Rechercher dans les logs',['⌘','F']],['Télécharger les logs',['⌘','S']],['Effacer le terminal',['⌘','L']]]],
];
const Kbd=({k})=><span style={{minWidth:24,height:24,padding:'0 7px',boxSizing:'border-box',display:'inline-flex',alignItems:'center',justifyContent:'center',borderRadius:7,background:'linear-gradient(180deg,#26262c,#1a1a1f)',border:'1px solid rgba(255,255,255,.12)',boxShadow:'0 2px 0 rgba(0,0,0,.5), inset 0 1px 0 rgba(255,255,255,.06)',font:'500 11px/1 var(--font-sans)',color:'var(--text-strong)'}}>{k}</span>;
function ShortcutsPage(){
  const [q,setQ]=React.useState('');
  return <div style={{padding:'20px 32px 40px 34px',display:'flex',flexDirection:'column',gap:16}}>
    <Banner col="56,189,248" grad="linear-gradient(110deg,#0e7490,#0ea5e9 55%,#38bdf8)" icon="keyboard" title="Allez plus vite au clavier" sub="Appuyez sur ⌘ / n'importe où pour afficher cette page." right={<div style={{display:'flex',alignItems:'center',gap:8,height:38,width:260,padding:'0 12px',borderRadius:10,background:'transparent',border:'1px solid var(--border-default)'}}><Icon name="search" size={14} color="var(--text-subtle)"/><input value={q} onChange={e=>setQ(e.target.value)} placeholder="Rechercher un raccourci" style={{flex:1,minWidth:0,background:'transparent',border:0,outline:'none',color:'var(--text-strong)',font:'400 13px/1 var(--font-sans)'}}/></div>}/>
    <div style={{display:'grid',gridTemplateColumns:'repeat(auto-fill,minmax(380px,1fr))',gap:12}}>
      {SC.map(([g,ic,col,items])=>{const list=items.filter(x=>x[0].toLowerCase().includes(q.toLowerCase()));if(!list.length)return null;return <div key={g} style={{...box,overflow:'hidden'}}>
        <div style={{display:'flex',alignItems:'center',gap:10,padding:'14px 18px',borderBottom:line}}><span style={{width:28,height:28,borderRadius:8,display:'flex',alignItems:'center',justifyContent:'center',background:'linear-gradient(180deg,#1f1f24,#16161a)',border:'1px solid rgba(255,255,255,.1)'}}><Icon name={ic} size={13} color="var(--text-strong)"/></span><span style={{font:'600 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{g}</span></div>
        {list.map(([n,keys],i)=><div key={n} style={{display:'flex',alignItems:'center',gap:12,minHeight:46,padding:'0 18px',borderTop:i?line:'none'}}><span style={{flex:1,font:'400 13px/1 var(--font-sans)',color:'var(--text-body)'}}>{n}</span><span style={{display:'flex',alignItems:'center',gap:4}}>{keys.map((k,j)=><React.Fragment key={j}>{j>0&&keys[0]==='G'||j>0&&keys[0]==='N'?<span style={{...lbl,fontSize:10,margin:'0 2px'}}>puis</span>:null}<Kbd k={k}/></React.Fragment>)}</span></div>)}
      </div>})}
    </div>
  </div>;
}

/* ---------- Documentation ---------- */
const DOC_CATS=[['rocket','Démarrage','Créer un espace, connecter GitHub, premier déploiement.','139,124,246',6],['layers','Projets','Builds, variables, domaines et previews.','125,211,252',12],['server','Serveurs','Installer l\'agent, clés SSH, monitoring.','134,239,172',9],['globe','Domaines','DNS Cloudflare, certificats, redirections.','243,128,32',7],['shield-half','Sécurité','CrowdSec, bannissements, WAF.','248,113,113',8],['code','API','Jetons, endpoints, webhooks et SDK.','251,191,36',15]];
const ARTS=[['Déployer un projet depuis GitHub','Démarrage','4 min',['Connectez GitHub dans Connecteurs.','Cliquez sur Importer un dépôt dans Projets.','Ciphera détecte le framework (Railpack) ou utilise votre Dockerfile.','Chaque push sur la branche de production déclenche un déploiement.']],['Installer l\'agent sur un serveur','Serveurs','3 min',['Dans Serveurs, cliquez sur Ajouter un serveur.','Copiez la commande d\'installation et exécutez-la en root.','L\'agent se connecte en QUIC + mTLS sur le port 7443/udp.']],['Connecter un domaine avec Cloudflare','Domaines','5 min',['Connectez Cloudflare dans Connecteurs.','Dans Domaines, ajoutez un domaine et choisissez la zone.','L\'enregistrement DNS et le certificat sont créés automatiquement.']],['Variables d\'environnement et secrets','Projets','3 min',['Les variables sont définies par environnement.','Les secrets sont chiffrés et masqués dans les logs.','Les variables globales sont héritées par tous les projets.']],['Authentifier l\'API avec un jeton','API','2 min',['Créez un jeton dans Clés API.','Envoyez-le dans l\'en-tête Authorization: Bearer <jeton>.']]];
function DocsPage(){
  const [q,setQ]=React.useState('');const [cat,setCat]=React.useState(null);const [art,setArt]=React.useState(null);
  const list=ARTS.filter(a=>(!cat||a[1]===cat)&&a[0].toLowerCase().includes(q.toLowerCase()));
  if(art){const a=ARTS.find(x=>x[0]===art);return <div style={{padding:'20px 32px 40px 34px',display:'flex',flexDirection:'column',gap:16,maxWidth:820}}>
    <button onClick={()=>setArt(null)} style={{display:'flex',alignItems:'center',gap:6,width:'fit-content',padding:0,border:0,background:'transparent',cursor:'pointer',font:'500 13px/1 var(--font-sans)',color:'var(--text-muted)'}}><Icon name="arrow-left" size={14}/>Documentation</button>
    <div style={{...box,padding:'28px 32px',display:'flex',flexDirection:'column',gap:16}}>
      <span style={{display:'flex',gap:8,...lbl}}><span>{a[1]}</span>·<span>{a[2]} de lecture</span></span>
      <span style={{font:'600 24px/1.2 var(--font-sans)',color:'var(--text-strong)',letterSpacing:'-0.01em'}}>{a[0]}</span>
      <div style={{display:'flex',flexDirection:'column',gap:14,marginTop:6}}>{a[3].map((s,i)=><div key={i} style={{display:'flex',gap:14}}><span style={{width:26,height:26,borderRadius:99,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'rgba(255,255,255,.06)',font:'600 12px/1 var(--font-sans)',color:'var(--text-strong)'}}>{i+1}</span><span style={{font:'400 14px/1.7 var(--font-sans)',color:'var(--text-body)',paddingTop:2}}>{s}</span></div>)}</div>
      <div style={{display:'flex',alignItems:'center',gap:10,marginTop:10,paddingTop:16,borderTop:line}}><span style={{flex:1,...lbl}}>Cet article vous a-t-il aidé ?</span><Button size="sm" variant="ghost" icon="thumbs-up">Oui</Button><Button size="sm" variant="ghost" icon="thumbs-down">Non</Button></div>
    </div>
  </div>}
  return <div style={{padding:'20px 32px 40px 34px',display:'flex',flexDirection:'column',gap:16}}>
    <div style={{position:'relative',display:'flex',flexDirection:'column',alignItems:'center',textAlign:'center',gap:14,padding:'38px 24px 34px',borderRadius:18,overflow:'hidden',border:'1px solid rgba(255,255,255,.1)',background:'#0f0f13'}}>
      <span style={{position:'absolute',inset:0,background:'radial-gradient(50% 120% at 20% 0%,rgba(139,124,246,.25),transparent 60%),radial-gradient(40% 120% at 85% 100%,rgba(56,189,248,.18),transparent 60%)'}}></span>
      <img src="../../web/src/assets/brand/ciphera-icon.svg" alt="" style={{position:'relative',width:38}}/>
      <span style={{position:'relative',font:'600 26px/1.2 var(--font-sans)',color:'#fff',letterSpacing:'-0.02em'}}>Comment pouvons-nous vous aider ?</span>
      <div style={{position:'relative',display:'flex',alignItems:'center',gap:10,height:46,width:'100%',maxWidth:520,padding:'0 16px',borderRadius:14,background:'rgba(255,255,255,.07)',border:'1px solid rgba(255,255,255,.14)'}}><Icon name="search" size={16} color="var(--text-muted)"/><input value={q} onChange={e=>setQ(e.target.value)} placeholder="Rechercher dans la documentation…" style={{flex:1,minWidth:0,background:'transparent',border:0,outline:'none',color:'#fff',font:'400 14px/1 var(--font-sans)'}}/><span style={{height:22,display:'flex',alignItems:'center',padding:'0 7px',borderRadius:6,background:'rgba(255,255,255,.1)',font:'500 11px/1 var(--font-mono)',color:'var(--text-body)'}}>⌘K</span></div>
    </div>
    <div style={{display:'grid',gridTemplateColumns:'repeat(auto-fill,minmax(260px,1fr))',gap:12}}>
      {DOC_CATS.map(([ic,n,d,col,cnt])=>{const on=cat===n;return <button key={n} onClick={()=>setCat(on?null:n)} style={{...box,display:'flex',flexDirection:'column',gap:12,padding:18,cursor:'pointer',textAlign:'left',borderColor:on?'rgba(255,255,255,.3)':'rgba(255,255,255,.08)',background:on?'rgba(255,255,255,.04)':'#0c0c0f'}}>
        <span style={{display:'flex',alignItems:'center',justifyContent:'space-between'}}><span style={{width:36,height:36,borderRadius:10,display:'flex',alignItems:'center',justifyContent:'center',background:'linear-gradient(180deg,#1f1f24,#16161a)',border:'1px solid rgba(255,255,255,.1)'}}><Icon name={ic} size={16} color="var(--text-strong)"/></span><span style={{...lbl,fontSize:11}}>{cnt} articles</span></span>
        <span style={{font:'600 14px/1 var(--font-sans)',color:'var(--text-strong)'}}>{n}</span><span style={{font:'400 12px/1.5 var(--font-sans)',color:'var(--text-muted)'}}>{d}</span></button>})}
    </div>
    <div style={{...box,overflow:'hidden'}}>
      <div style={{display:'flex',alignItems:'center',padding:'14px 18px',borderBottom:line}}><span style={{font:'600 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{cat?cat:'Articles populaires'}</span>{cat&&<button onClick={()=>setCat(null)} style={{marginLeft:'auto',padding:0,border:0,background:'transparent',cursor:'pointer',...lbl}}>Tout afficher</button>}</div>
      {list.map((a,i)=><button key={a[0]} onClick={()=>setArt(a[0])} style={{display:'flex',alignItems:'center',gap:12,width:'100%',minHeight:50,padding:'0 18px',border:0,borderTop:i?line:'none',background:'transparent',cursor:'pointer',textAlign:'left'}}><Icon name="file-text" size={14} color="var(--text-muted)"/><span style={{flex:1,font:'400 13px/1 var(--font-sans)',color:'var(--text-body)'}}>{a[0]}</span><span style={{...lbl,fontSize:11}}>{a[1]} · {a[2]}</span><Icon name="chevron-right" size={14} color="var(--text-subtle)"/></button>)}
      {!list.length&&<div style={{padding:'30px 18px',textAlign:'center',...lbl}}>Aucun article ne correspond.</div>}
    </div>
  </div>;
}
Object.assign(window,{WebhooksPage,ShortcutsPage,DocsPage});
})();
