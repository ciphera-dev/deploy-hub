(()=>{
const {Button,IconButton,Badge,Icon,Switch,Select,StatusDot,Modal}=window.CipheraDesignSystem;
const hair='1px solid var(--border-subtle)';
const lbl={font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'};
const mono=(v,c)=><span style={{fontFamily:'var(--font-mono)',fontSize:12,color:c}}>{v}</span>;
const field={height:32,padding:'0 10px',borderRadius:'var(--radius-md)',border:'1px solid var(--border-strong)',background:'var(--surface-control)',color:'var(--text-strong)',font:'400 12px/1 var(--font-mono)',outline:'none',minWidth:0,boxShadow:'var(--shadow-control)'};

function Section({title,desc,children,last}){
  return <div style={{display:'grid',gridTemplateColumns:'260px minmax(0,640px)',gap:40,padding:'28px 12px',borderBottom:last?'none':hair}}>
    <div style={{display:'flex',flexDirection:'column',gap:8}}><span style={{font:'500 14px/1.2 var(--font-sans)',color:'var(--text-strong)'}}>{title}</span><span style={{font:'400 12px/1.5 var(--font-sans)',color:'var(--text-subtle)',textWrap:'pretty'}}>{desc}</span></div>
    <div style={{display:'flex',flexDirection:'column',gap:16,minWidth:0}}>{children}</div>
  </div>;
}
function Field({label,hint,children}){return <div style={{display:'flex',flexDirection:'column',gap:8,flex:1,minWidth:0}}><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>{label}</span>{children}{hint&&<span style={{...lbl,fontSize:11,color:'var(--text-faint)'}}>{hint}</span>}</div>}
function Chips({items,onChange,placeholder}){
  const [v,setV]=React.useState('');
  return <div style={{...field,height:'auto',minHeight:32,display:'flex',flexWrap:'wrap',alignItems:'center',gap:6,padding:'4px 6px'}}>
    {items.map(t=><span key={t} style={{display:'flex',alignItems:'center',gap:4,height:22,padding:'0 4px 0 8px',borderRadius:5,background:'var(--surface-selected)',font:'400 12px/1 var(--font-mono)',color:'var(--text-strong)'}}>{t}<button onClick={()=>onChange(items.filter(x=>x!==t))} style={{display:'flex',background:'none',border:0,padding:2,cursor:'pointer',color:'var(--text-subtle)'}}><Icon name="x" size={11}/></button></span>)}
    <input value={v} onChange={e=>setV(e.target.value)} onKeyDown={e=>{if((e.key==='Enter'||e.key===',')&&v.trim()){e.preventDefault();if(!items.includes(v.trim()))onChange([...items,v.trim()]);setV('')}if(e.key==='Backspace'&&!v&&items.length)onChange(items.slice(0,-1))}} placeholder={items.length?'':placeholder} style={{flex:1,minWidth:80,height:22,background:'transparent',border:0,outline:'none',color:'var(--text-strong)',font:'400 12px/1 var(--font-mono)',padding:'0 4px'}}/>
  </div>;
}
function Copy({text,flash}){return <IconButton size={28} variant="ghost" icon="copy" label="Copier" onClick={()=>{navigator.clipboard&&navigator.clipboard.writeText(text);flash('Copié')}}/>}

const DELIVERIES=[
  {ev:'push',ref:'main · c81e0a9',code:200,ms:84,when:'il y a 2 min'},
  {ev:'pull_request.opened',ref:'#214 feat/billing-v2',code:200,ms:91,when:'il y a 47 min'},
  {ev:'push',ref:'feat/logs · e19b44d',code:200,ms:77,when:'il y a 6 h'},
  {ev:'pull_request.closed',ref:'#209 fix/cors',code:200,ms:69,when:'hier'},
  {ev:'push',ref:'main · 9fa3c10',code:502,ms:30012,when:'il y a 2 j'},
];

function ProjectGit({project='ciphera-web',repo='github.com/ciphera/web',branch='main'}){
  const init={branch,root:'./',previews:['feat/*','fix/*'],watch:['apps/web/**','packages/ui/**'],auto:true,prev:true,comment:true,skip:true,ignoreBots:false};
  const [s,setS]=React.useState(init);const [saved,setSaved]=React.useState(init);
  const [reveal,setReveal]=React.useState(false);const [secret,setSecret]=React.useState('whsec_9f2a7c41e0b84d1cb63a');
  const [hooks,setHooks]=React.useState([{name:'CI externe',branch:'main',url:'https://api.ciphera.dev/v1/hooks/deploy/dh_7Kq2xP9mVb'}]);
  const [dl,setDl]=React.useState(DELIVERIES);
  const [confirm,setConfirm]=React.useState(null);
  const [toast,setToast]=React.useState('');
  const flash=m=>{setToast(m);clearTimeout(flash.t);flash.t=setTimeout(()=>setToast(''),2000)};
  const set=(k,v)=>setS(x=>({...x,[k]:v}));
  const dirty=JSON.stringify(s)!==JSON.stringify(saved);
  const prov=repo.startsWith('gitlab')?'gitlab':'github';
  const whUrl='https://api.ciphera.dev/v1/webhooks/'+prov+'/prj_web_4f2a';
  return <div style={{position:'relative',paddingBottom:dirty?72:0}}>
    <Section title="Dépôt" desc="Ciphera reçoit les événements push et pull request de ce dépôt via un webhook signé (HMAC SHA-256).">
      <div style={{borderRadius:'var(--radius-lg)',border:'1px solid var(--border-default)',overflow:'hidden'}}>
        <div style={{display:'flex',alignItems:'center',gap:14,padding:'16px 16px'}}>
          <span style={{width:40,height:40,borderRadius:10,display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-selected)',color:'var(--text-strong)',flex:'none'}}><Icon name={prov} size={20}/></span>
          <div style={{display:'flex',flexDirection:'column',gap:7,flex:1,minWidth:0}}>
            <div style={{display:'flex',alignItems:'center',gap:8}}><span style={{font:'500 14px/1 var(--font-sans)',color:'var(--text-strong)'}}>{repo.split('/').slice(1).join('/')}</span><Icon name="lock" size={11} color="var(--text-faint)"/><Icon name="arrow-up-right" size={13} color="var(--text-muted)"/></div>
            <span style={{...lbl,display:'flex',alignItems:'center',gap:8}}>{prov==='github'?'GitHub App · ciphera':'GitLab · ciphera-group'}<span style={{color:'var(--text-faint)'}}>·</span><StatusDot status="ok" label="Webhook actif"/></span>
          </div>
          <Button size="sm" onClick={()=>flash('Sélecteur de dépôt (Nouveau projet › Source)')}>Changer</Button>
          <Button size="sm" variant="ghost" onClick={()=>setConfirm('disconnect')}>Déconnecter</Button>
        </div>
        <div style={{display:'flex',alignItems:'center',gap:10,padding:'12px 16px',borderTop:hair,background:'var(--surface-raised)',font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'}}>
          <Icon name="git-commit-horizontal" size={13}/>Dernier commit sur {mono(saved.branch,'var(--text-body)')}
          <span style={{color:'var(--text-body)',flex:1,minWidth:0,overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'}}>feat: graphiques de latence</span>{mono('c81e0a9')}<span>il y a 2 min</span>
        </div>
      </div>
    </Section>
    <Section title="Branches" desc="La branche de production met à jour vos domaines. Les autres branches correspondant aux motifs obtiennent un déploiement de preview.">
      <div style={{display:'flex',gap:16}}>
        <Field label="Branche de production"><Select options={['main','develop','release']} value={s.branch} onChange={v=>set('branch',v)} style={{width:'100%'}}/></Field>
        <Field label="Dossier racine" hint="Monorepo : chemin de l'application"><input value={s.root} onChange={e=>set('root',e.target.value)} style={field}/></Field>
      </div>
      <Field label="Branches de preview" hint="Entrée pour ajouter · motifs glob acceptés"><Chips items={s.previews} onChange={v=>set('previews',v)} placeholder="feat/*"/></Field>
      <Field label="Chemins surveillés" hint="Un build n'est lancé que si un fichier correspondant change"><Chips items={s.watch} onChange={v=>set('watch',v)} placeholder="apps/web/**"/></Field>
    </Section>
    <Section title="Déclencheurs" desc="Contrôlez quand un push ou une pull request lance un build.">
      <div style={{borderRadius:'var(--radius-lg)',border:'1px solid var(--border-default)'}}>
        {[['auto','Déploiement automatique','Chaque push sur '+s.branch+' déclenche un build et un déploiement en production.'],['prev','Previews de pull request','Un environnement éphémère par PR, supprimé à la fusion ou à la fermeture.'],['comment','Commenter les pull requests','Le bot Ciphera publie l\'URL de preview et le statut du build dans la PR.'],['skip','Respecter [skip ci]','Ignore les commits dont le message contient [skip ci] ou [ciphera skip].'],['ignoreBots','Ignorer les commits de bots','Dependabot, Renovate et autres comptes *[bot].']].map(([k,t,d],i)=><div key={k} style={{padding:'14px 16px',borderTop:i?hair:'none'}}><Switch checked={s[k]} onChange={v=>set(k,v)} label={t} description={d}/></div>)}
      </div>
    </Section>
    <Section title="Webhook" desc="Configuré automatiquement par l'intégration. À copier manuellement pour un Gitea ou GitLab auto-hébergé.">
      <Field label="URL de payload"><div style={{...field,display:'flex',alignItems:'center',gap:8,paddingRight:2}}><span style={{flex:1,minWidth:0,overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'}}>{whUrl}</span><Copy text={whUrl} flash={flash}/></div></Field>
      <Field label="Secret de signature"><div style={{display:'flex',gap:8}}>
        <div style={{...field,flex:1,display:'flex',alignItems:'center',gap:4,paddingRight:2}}><span style={{flex:1,minWidth:0,overflow:'hidden'}}>{reveal?secret:'•'.repeat(24)}</span><IconButton size={28} variant="ghost" icon={reveal?'eye-off':'eye'} label="Afficher" onClick={()=>setReveal(r=>!r)}/><Copy text={secret} flash={flash}/></div>
        <Button icon="refresh-cw" onClick={()=>setConfirm('rotate')}>Régénérer</Button>
      </div></Field>
      <div style={{display:'flex',flexDirection:'column',gap:8}}>
        <span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>Livraisons récentes</span>
        <div style={{borderRadius:'var(--radius-lg)',border:'1px solid var(--border-default)',overflow:'hidden'}}>
          {dl.map((d,i)=><div key={i} style={{display:'grid',gridTemplateColumns:'54px minmax(0,1.1fr) minmax(0,1.3fr) 70px 90px 28px',gap:12,alignItems:'center',minHeight:42,padding:'0 6px 0 14px',borderTop:i?hair:'none'}}>
            <span style={{height:20,display:'flex',alignItems:'center',justifyContent:'center',borderRadius:5,font:'500 11px/1 var(--font-mono)',background:d.code<300?'rgba(34,192,138,.12)':'rgba(239,68,68,.12)',color:d.code<300?'var(--green-500)':'#f87171'}}>{d.code}</span>
            <span style={{font:'500 12px/1 var(--font-mono)',color:'var(--text-strong)',overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'}}>{d.ev}</span>
            <span style={{...lbl,overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'}}>{d.ref}</span>
            <span style={{...lbl,fontFamily:'var(--font-mono)',fontSize:11,textAlign:'right'}}>{d.ms>9999?'timeout':d.ms+' ms'}</span>
            <span style={{...lbl,textAlign:'right'}}>{d.when}</span>
            <IconButton size={26} variant="ghost" icon="rotate-cw" label="Relivrer" onClick={()=>{setDl(x=>[{...d,code:200,ms:72,when:"à l'instant"},...x]);flash('Événement relivré')}}/>
          </div>)}
        </div>
      </div>
    </Section>
    <Section title="Deploy hooks" desc="URL secrète qui déclenche un déploiement par simple POST, depuis une CI externe ou un CMS." last>
      {hooks.map((h,i)=><div key={i} style={{display:'flex',alignItems:'center',gap:12,padding:'12px 8px 12px 14px',borderRadius:'var(--radius-lg)',border:'1px solid var(--border-default)'}}>
        <Icon name="webhook" size={15} color="var(--text-muted)"/>
        <div style={{display:'flex',flexDirection:'column',gap:6,flex:1,minWidth:0}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{h.name} <span style={{...lbl,fontWeight:400}}>· {h.branch}</span></span><span style={{font:'400 11px/1 var(--font-mono)',color:'var(--text-subtle)',overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'}}>{h.url}</span></div>
        <Copy text={'curl -X POST '+h.url} flash={flash}/><IconButton size={28} variant="ghost" icon="trash-2" label="Supprimer" onClick={()=>setHooks(x=>x.filter((_,j)=>j!==i))}/>
      </div>)}
      <div><Button icon="plus" onClick={()=>{setHooks(x=>[...x,{name:'Hook '+(x.length+1),branch:s.branch,url:'https://api.ciphera.dev/v1/hooks/deploy/dh_'+Math.random().toString(36).slice(2,12)}]);flash('Deploy hook créé')}}>Créer un deploy hook</Button></div>
    </Section>
    {dirty&&<div style={{position:'sticky',bottom:16,marginTop:-56,display:'flex',justifyContent:'center',pointerEvents:'none'}}>
      <div style={{pointerEvents:'auto',display:'flex',alignItems:'center',gap:12,padding:'8px 8px 8px 16px',borderRadius:12,background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)'}}>
        <span style={{width:6,height:6,borderRadius:99,background:'#e5b454'}}></span><span style={{font:'400 13px/1 var(--font-sans)',color:'var(--text-body)'}}>Modifications non enregistrées</span>
        <Button size="sm" variant="ghost" onClick={()=>setS(saved)}>Annuler</Button><Button size="sm" variant="inverse" onClick={()=>{setSaved(s);flash('Paramètres Git enregistrés')}}>Enregistrer</Button>
      </div>
    </div>}
    {confirm==='rotate'&&<Modal title="Régénérer le secret ?" description="Les livraisons signées avec l'ancien secret seront refusées. L'intégration GitHub est mise à jour automatiquement." onClose={()=>setConfirm(null)} width={460} footer={<><Button onClick={()=>setConfirm(null)}>Annuler</Button><Button variant="inverse" onClick={()=>{setSecret('whsec_'+Math.random().toString(16).slice(2,22));setReveal(true);setConfirm(null);flash('Nouveau secret généré')}}>Régénérer</Button></>}></Modal>}
    {confirm==='disconnect'&&<Modal title="Déconnecter le dépôt ?" description={'Les pushs sur '+repo+' ne déclencheront plus de déploiement. Le projet reste en ligne avec la dernière version.'} onClose={()=>setConfirm(null)} width={460} footer={<><Button onClick={()=>setConfirm(null)}>Annuler</Button><Button variant="inverse" onClick={()=>{setConfirm(null);flash('Dépôt déconnecté')}}>Déconnecter</Button></>}></Modal>}
    {toast&&<div style={{position:'fixed',bottom:24,right:24,zIndex:60,padding:'10px 14px',borderRadius:'var(--radius-md)',background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)',font:'400 12px/1 var(--font-sans)',color:'var(--text-strong)',display:'flex',alignItems:'center',gap:8}}><Icon name="check" size={12} color="var(--green-500)"/>{toast}</div>}
  </div>;
}
window.ProjectGit=ProjectGit;
})();
