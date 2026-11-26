(()=>{
const {Button,Icon,Switch,Modal}=window.CipheraDesignSystem;
const hair='1px solid var(--border-subtle)';
const card={borderRadius:16,border:'1px solid var(--border-default)',background:'var(--surface-app)',overflow:'hidden'};
const SUB=[['info','user-round','Informations du compte'],['security','shield-check','Sécurité'],['activity','history','Activité du compte'],['notif','bell','Notifications']];
const Ib=({icon,label,onClick,danger})=><button title={label} onClick={onClick} onMouseEnter={e=>{e.currentTarget.style.background='var(--surface-selected)';e.currentTarget.style.borderColor='var(--gray-600)'}} onMouseLeave={e=>{e.currentTarget.style.background='transparent';e.currentTarget.style.borderColor='var(--border-strong)'}} style={{width:32,height:32,borderRadius:99,border:'1px solid var(--border-strong)',transition:'all .15s',background:'transparent',cursor:'pointer',display:'flex',alignItems:'center',justifyContent:'center',color:danger?'#f87171':'var(--text-body)'}}><Icon name={icon} size={14}/></button>;
const TINT={'id-card':'#8b6cff','user-round-cog':'#38bdf8','key-round':'#f59e0b','monitor-smartphone':'#22c08a','triangle-alert':'#ef4444','history':'#ec4899','bell':'#f97316'};
const rgba=(h,a)=>{const n=parseInt(h.slice(1),16);return 'rgba('+(n>>16&255)+','+(n>>8&255)+','+(n&255)+','+a+')'};
function Card({icon,title,desc,children,action}){const c=TINT[icon]||'#8b6cff';return <div style={{...card,position:'relative'}}>
  <div style={{display:'flex',alignItems:'center',gap:12,padding:'16px 20px',borderBottom:hair}}><span style={{width:34,height:34,borderRadius:10,display:'flex',alignItems:'center',justifyContent:'center',background:'linear-gradient(180deg,#232327,#141417)',border:'1px solid rgba(255,255,255,.08)',boxShadow:'inset 0 1px 0 rgba(255,255,255,.07), 0 1px 2px rgba(0,0,0,.4)',flex:'none'}}><Icon name={icon} size={16} color="#ededed"/></span><span style={{display:'flex',flexDirection:'column',gap:6,flex:1,minWidth:0}}><span style={{font:'600 15px/1 var(--font-sans)',color:'var(--text-strong)'}}>{title}</span>{desc&&<span style={{font:'400 12px/1.3 var(--font-sans)',color:'var(--text-subtle)'}}>{desc}</span>}</span>{action}</div>
  {children}</div>}
function Row({label,value,onSave,onDelete,mono,info,type='text',last,render}){
  const [ed,setEd]=React.useState(false);const [v,setV]=React.useState(value);
  React.useEffect(()=>setV(value),[value]);
  return <div style={{display:'grid',gridTemplateColumns:'220px minmax(0,1fr) 76px',gap:16,alignItems:'center',minHeight:58,padding:'0 16px 0 20px',borderBottom:last?'none':hair}}>
    <span style={{display:'flex',alignItems:'center',gap:6,font:'400 13px/1 var(--font-sans)',color:'var(--text-muted)',whiteSpace:'nowrap'}}>{label}{info&&<span title={info} style={{display:'flex'}}><Icon name="info" size={13} color="var(--text-subtle)"/></span>}</span>
    {ed?<input autoFocus type={type} value={v} onChange={e=>setV(e.target.value)} onKeyDown={e=>{if(e.key==='Enter'){onSave(v);setEd(false)}if(e.key==='Escape'){setV(value);setEd(false)}}} style={{height:36,padding:'0 12px',borderRadius:9,border:'1px solid var(--gray-600)',background:'var(--gray-0)',color:'var(--text-strong)',font:'400 13px/1 '+(mono?'var(--font-mono)':'var(--font-sans)'),outline:'none',minWidth:0}}/>
      :render||<span style={{font:'400 13px/1.3 '+(mono?'var(--font-mono)':'var(--font-sans)'),color:value?'var(--text-strong)':'var(--text-faint)',overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'}}>{type==='password'?'••••••••••••':value||'—'}</span>}
    <div style={{display:'flex',gap:8,justifyContent:'flex-end',minWidth:32}}>
      {ed?<><Ib icon="x" label="Annuler" onClick={()=>{setV(value);setEd(false)}}/><button onClick={()=>{onSave(v);setEd(false)}} style={{width:32,height:32,borderRadius:99,border:0,background:'#ededed',color:'#0b0b0b',cursor:'pointer',display:'flex',alignItems:'center',justifyContent:'center'}}><Icon name="check" size={14}/></button></>
      :<>{onDelete&&value&&<Ib icon="trash-2" label="Supprimer" onClick={onDelete}/>}{onSave&&<Ib icon="pencil" label="Modifier" onClick={()=>{setV(type==='password'?'':value);setEd(true)}}/>}</>}
    </div>
  </div>;
}
function Info({p,set,flash}){
  const u=k=>v=>{set({...p,[k]:v});flash('Modifié')};
  return <>
    <div style={{...card,position:'relative'}}>
      <div style={{position:'relative',height:132,overflow:'hidden',background:'radial-gradient(60% 140% at 0% 0%,#4c3a9e 0%,transparent 60%),radial-gradient(50% 120% at 100% 0%,#1e4e8c 0%,transparent 65%),radial-gradient(40% 90% at 60% 100%,#6b2a6e 0%,transparent 70%),#17142a'}}>
        <div style={{position:'absolute',inset:0,backgroundImage:'linear-gradient(rgba(255,255,255,.045) 1px,transparent 1px),linear-gradient(90deg,rgba(255,255,255,.045) 1px,transparent 1px)',backgroundSize:'28px 28px',maskImage:'linear-gradient(180deg,rgba(0,0,0,.9),transparent)',WebkitMaskImage:'linear-gradient(180deg,rgba(0,0,0,.9),transparent)'}}></div>
        <div style={{position:'absolute',inset:0,background:'linear-gradient(180deg,transparent 55%,var(--surface-app))'}}></div>
      </div>
      <div style={{display:'flex',alignItems:'flex-end',gap:18,padding:'0 22px 20px',marginTop:-46,position:'relative'}}>
        <span style={{position:'relative',padding:4,borderRadius:99,background:'var(--surface-app)'}}><window.CipheraAvatar size={88} editable ring="var(--surface-app)" dot={false}/><span style={{position:'absolute',right:2,bottom:2,width:28,height:28,borderRadius:99,background:'#ededed',color:'#0b0b0b',display:'flex',alignItems:'center',justifyContent:'center',boxShadow:'0 0 0 3px var(--surface-app)',pointerEvents:'none'}}><Icon name="camera" size={13}/></span></span>
        <div style={{display:'flex',flexDirection:'column',gap:9,flex:1,minWidth:0,paddingBottom:6}}>
          <span style={{display:'flex',alignItems:'center',gap:10}}><span style={{font:'600 22px/1.1 var(--font-sans)',color:'var(--text-strong)',letterSpacing:'-0.01em'}}>{p.name}</span><span style={{display:'flex',alignItems:'center',gap:5,height:22,padding:'0 9px',borderRadius:99,background:'rgba(229,180,84,.12)',border:'1px solid rgba(229,180,84,.3)',color:'#e5b454',font:'500 11px/1 var(--font-sans)'}}><Icon name="crown" size={11}/>Propriétaire</span></span>
          <span style={{display:'flex',alignItems:'center',gap:14,font:'400 13px/1 var(--font-sans)',color:'var(--text-subtle)'}}><span style={{display:'flex',alignItems:'center',gap:6}}><Icon name="mail" size={13}/>{p.email}</span><span style={{display:'flex',alignItems:'center',gap:6}}><Icon name="map-pin" size={13}/>Genève, CH</span><span style={{display:'flex',alignItems:'center',gap:6}}><Icon name="calendar" size={13}/>Depuis mars 2024</span></span>
        </div>
      </div>
      <div style={{display:'grid',gridTemplateColumns:'repeat(4,minmax(0,1fr))',borderTop:hair}}>
        {[['layers','Projets','12','#8b6cff'],['rocket','Déploiements (30 j)','184','#38bdf8'],['server','Serveurs','4','#22c08a'],['shield-check','Sécurité du compte','92 %','#f59e0b']].map(([ic,k,v,c],i)=><div key={k} style={{display:'flex',alignItems:'center',gap:12,padding:'14px 20px',borderLeft:i?hair:'none'}}>
          <span style={{width:32,height:32,borderRadius:9,display:'flex',alignItems:'center',justifyContent:'center',background:'linear-gradient(180deg,#232327,#141417)',border:'1px solid rgba(255,255,255,.08)',boxShadow:'inset 0 1px 0 rgba(255,255,255,.07), 0 1px 2px rgba(0,0,0,.4)'}}><Icon name={ic} size={15} color="#ededed"/></span>
          <span style={{display:'flex',flexDirection:'column',gap:6}}><span style={{font:'600 17px/1 var(--font-sans)',color:'var(--text-strong)'}}>{v}</span><span style={{font:'400 11px/1 var(--font-sans)',color:'var(--text-subtle)'}}>{k}</span></span>
        </div>)}
      </div>
    </div>
    <Card icon="id-card" title="Données personnelles" desc="Figurent sur vos factures">
      <Row label="Nom" value={p.name} onSave={u('name')}/>
      <Row label="Adresse" value={p.address} onSave={u('address')}/>
      <Row label="Numéro de téléphone" value={p.phone} type="tel" onSave={u('phone')} onDelete={()=>{set({...p,phone:''});flash('Numéro supprimé')}}/>
      <Row label="Entreprise" value={p.company} onSave={u('company')}/>
      <Row label="Devise du compte" value="EUR" info="Définie à la création du compte" last/>
    </Card>
    <Card icon="user-round-cog" title="Paramètres du compte" desc="Connexion et préférences">
      <Row label="Adresse e-mail" value={p.email} type="email" onSave={u('email')}/>
      <Row label="E-mail de récupération" value={p.recovery} type="email" onSave={u('recovery')} onDelete={()=>{set({...p,recovery:''});flash('Supprimé')}}/>
      <Row label="Langue" value="Français" render={<select defaultValue="fr" style={{height:34,padding:'0 10px',borderRadius:9,border:'1px solid var(--border-strong)',background:'var(--surface-control)',color:'var(--text-strong)',colorScheme:'dark',width:200,font:'400 13px/1 var(--font-sans)',outline:'none'}}><option value="fr">Français</option><option>English</option><option>Deutsch</option></select>}/>
      <Row label="Fuseau horaire" value="Europe/Zurich (UTC+1)" last/>
    </Card>
  </>;
}
function Security({flash}){
  const [mfa,setMfa]=React.useState(true);const [keys,setKeys]=React.useState(true);const [sess,setSess]=React.useState([['laptop','MacBook Pro · Chrome','Genève, CH · 185.12.4.20','Cette session',true],['smartphone','iPhone 15 · Safari','Genève, CH · 185.12.4.20','il y a 2 h'],['monitor','Windows · Firefox','Lyon, FR · 92.184.10.3','il y a 3 j']]);
  return <>
    <Card icon="key-round" title="Connexion">
      <Row label="Mot de passe" value="x" type="password" onSave={()=>flash('Mot de passe modifié')} info="Modifié il y a 2 mois"/>
      <Row label="Double authentification" value="" render={<span style={{display:'flex',alignItems:'center',gap:8,font:'400 13px/1 var(--font-sans)',color:mfa?'var(--text-strong)':'#e5b454'}}><Icon name={mfa?'shield-check':'shield-alert'} size={14} color={mfa?'var(--green-500)':'#e5b454'}/>{mfa?'Activée · application TOTP':'Désactivée'}</span>}/>
      <div style={{padding:'14px 20px',borderBottom:hair}}><Switch checked={mfa} onChange={v=>{setMfa(v);flash(v?'2FA activée':'2FA désactivée')}} label="Exiger un code à chaque connexion" description="Google Authenticator, 1Password, Authy…"/></div>
      <div style={{padding:'14px 20px'}}><Switch checked={keys} onChange={setKeys} label="Clés de sécurité (WebAuthn)" description="YubiKey ou Touch ID · 1 clé enregistrée"/></div>
    </Card>
    <Card icon="monitor-smartphone" title="Sessions actives" action={<Button size="sm" onClick={()=>{setSess(s=>s.filter(x=>x[4]));flash('Autres sessions déconnectées')}}>Tout déconnecter</Button>}>
      {sess.map((x,i)=><div key={x[1]} style={{display:'flex',alignItems:'center',gap:14,padding:'14px 16px 14px 20px',borderTop:i?hair:'none'}}>
        <span style={{width:36,height:36,borderRadius:10,display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-selected)'}}><Icon name={x[0]} size={16}/></span>
        <span style={{flex:1,display:'flex',flexDirection:'column',gap:6}}><span style={{display:'flex',alignItems:'center',gap:8,font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{x[1]}{x[4]&&<span style={{height:18,display:'flex',alignItems:'center',padding:'0 7px',borderRadius:99,background:'rgba(34,192,138,.14)',color:'var(--green-500)',font:'500 11px/1 var(--font-sans)'}}>Actuelle</span>}</span><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'}}>{x[2]} · {x[3]}</span></span>
        {!x[4]&&<Ib icon="log-out" label="Déconnecter" danger onClick={()=>{setSess(s=>s.filter(y=>y!==x));flash('Session déconnectée')}}/>}
      </div>)}
    </Card>
    <Card icon="triangle-alert" title="Zone sensible">
      <div style={{display:'flex',alignItems:'center',gap:16,padding:'16px 20px'}}><span style={{flex:1,display:'flex',flexDirection:'column',gap:6}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>Supprimer mon compte</span><span style={{font:'400 12px/1.4 var(--font-sans)',color:'var(--text-subtle)'}}>Transférez d'abord la propriété de vos espaces.</span></span><button onClick={()=>flash('Transférez la propriété avant de supprimer')} style={{height:34,padding:'0 14px',borderRadius:'var(--radius-md)',border:'1px solid rgba(239,68,68,.4)',background:'rgba(239,68,68,.08)',color:'#f87171',cursor:'pointer',font:'500 13px/1 var(--font-sans)'}}>Supprimer</button></div>
    </Card>
  </>;
}
function Activity(){
  const L=[['log-in','Connexion réussie','MacBook Pro · Genève','il y a 4 min'],['rocket','A déployé ciphera-web #4821','Production','il y a 2 h'],['key-round','A créé le jeton API proud-river','Accès complet','hier'],['shield-check','Double authentification activée','Application TOTP','il y a 3 j'],['globe','A ajouté api.ciphera.com','Cloudflare','il y a 5 j'],['log-in','Échec de connexion','Lyon, FR · mot de passe erroné','il y a 1 sem.',1]];
  return <Card icon="history" title="Activité du compte" desc="Les 90 derniers jours de votre activité.">
    {L.map((x,i)=><div key={i} style={{display:'flex',alignItems:'center',gap:14,padding:'14px 20px',borderTop:i?hair:'none'}}>
      <span style={{width:32,height:32,borderRadius:99,display:'flex',alignItems:'center',justifyContent:'center',background:x[4]?'rgba(239,68,68,.12)':'var(--surface-selected)'}}><Icon name={x[0]} size={14} color={x[4]?'#f87171':'var(--text-body)'}/></span>
      <span style={{flex:1,display:'flex',flexDirection:'column',gap:6}}><span style={{font:'500 13px/1 var(--font-sans)',color:x[4]?'#f87171':'var(--text-strong)'}}>{x[1]}</span><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'}}>{x[2]}</span></span>
      <span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'}}>{x[3]}</span>
    </div>)}
  </Card>;
}
function Notif(){
  const [n,setN]=React.useState({dep:[1,1],fail:[1,1],sec:[1,1],dns:[0,1],team:[1,0],week:[1,0]});
  const G=[['Déploiements',[['dep','circle-check','#22c08a','Déploiement réussi','Quand une version passe en production'],['fail','circle-x','#ef4444','Échec de build ou de déploiement','Avec les dernières lignes de log']]],['Infrastructure',[['sec','shield-alert','#f59e0b','Alertes de sécurité','Attaques bloquées, IP bannies, connexions suspectes'],['dns','globe','#38bdf8','Certificats et DNS','Expiration, renouvellement, erreurs Cloudflare']]],['Organisation',[['team','users','#8b6cff','Équipe','Invitations, changements de rôle'],['week','calendar','#ec4899','Rapport hebdomadaire','Résumé chaque lundi matin']]]];
  const Chip=({on,ic,label,onClick})=><button onClick={onClick} style={{display:'flex',alignItems:'center',gap:6,height:30,padding:'0 11px',borderRadius:99,cursor:'pointer',outline:'none',border:'1px solid '+(on?'transparent':'var(--border-strong)'),background:on?'#ededed':'transparent',color:on?'#0b0b0b':'var(--text-muted)',font:'500 12px/1 var(--font-sans)',transition:'all .15s'}}><Icon name={on?'check':ic} size={12}/>{label}</button>;
  const all=k=>Object.values(n).every(v=>v[k]);
  return <>
    <div style={{display:'flex',alignItems:'center',gap:12}}>
      <span style={{flex:1,font:'400 13px/1.4 var(--font-sans)',color:'var(--text-subtle)'}}>Choisissez où recevoir chaque événement.</span>
      <Chip on={all(0)} ic="mail" label="Tout par e-mail" onClick={()=>{const v=all(0)?0:1;setN(s=>Object.fromEntries(Object.entries(s).map(([k,x])=>[k,[v,x[1]]])))}}/>
      <Chip on={all(1)} ic="layout-dashboard" label="Tout sur le dashboard" onClick={()=>{const v=all(1)?0:1;setN(s=>Object.fromEntries(Object.entries(s).map(([k,x])=>[k,[x[0],v]])))}}/>
    </div>
    {G.map(([g,rows])=><div key={g} style={{display:'flex',flexDirection:'column',gap:8}}>
      <span style={{padding:'0 4px',font:'500 12px/1 var(--font-sans)',color:'var(--text-subtle)'}}>{g}</span>
      <div style={card}>{rows.map(([k,ic,c,t,d],i)=><div key={k} style={{display:'flex',alignItems:'center',gap:14,padding:'14px 16px 14px 18px',borderTop:i?hair:'none'}}>
        <span style={{width:36,height:36,borderRadius:10,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:rgba(c,.1)}}><Icon name={ic} size={16} color={c}/></span>
        <span style={{flex:1,display:'flex',flexDirection:'column',gap:6,minWidth:0}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{t}</span><span style={{font:'400 12px/1.3 var(--font-sans)',color:'var(--text-subtle)'}}>{d}</span></span>
        <div style={{display:'flex',gap:6}}><Chip on={!!n[k][0]} ic="mail" label="E-mail" onClick={()=>setN(s=>({...s,[k]:[s[k][0]?0:1,s[k][1]]}))}/><Chip on={!!n[k][1]} ic="layout-dashboard" label="Dashboard" onClick={()=>setN(s=>({...s,[k]:[s[k][0],s[k][1]?0:1]}))}/></div>
      </div>)}</div>
    </div>)}
  </>;
}
function ProfilePage(){
  const [tab,setTab]=React.useState('info');
  const [p,setP]=React.useState({name:'Alex Martin',address:'1 rue Exemple, 1000 Lausanne, CH',phone:'+41 00 000 00 00',company:'Ciphera SAS',email:'alex@example.com',recovery:''});
  const [toast,setToast]=React.useState('');const flash=m=>{setToast(m);clearTimeout(flash.t);flash.t=setTimeout(()=>setToast(''),2000)};
  const T=SUB.find(s=>s[0]===tab);
  return <div style={{display:'grid',gridTemplateColumns:'240px minmax(0,1fr)',minHeight:'100%'}}>
    <div style={{padding:'20px 12px',borderRight:hair,display:'flex',flexDirection:'column',gap:4}}>
      <span style={{padding:'0 10px 12px',font:'500 12px/1 var(--font-sans)',color:'var(--text-subtle)'}}>Profil</span>
      {SUB.map(([k,ic,l],i)=>{const on=k===tab;const c=['#8b6cff','#f59e0b','#ec4899','#f97316'][i];return <button key={k} onClick={()=>setTab(k)} onMouseEnter={e=>{if(!on)e.currentTarget.style.background='rgba(255,255,255,.03)'}} onMouseLeave={e=>{if(!on)e.currentTarget.style.background='transparent'}} style={{position:'relative',display:'flex',alignItems:'center',gap:11,height:42,padding:'0 10px',borderRadius:11,border:0,outline:'none',cursor:'pointer',textAlign:'left',background:on?'var(--surface-selected)':'transparent',color:on?'var(--text-strong)':'var(--text-muted)',font:'500 13px/1 var(--font-sans)',transition:'background .15s'}}><span style={{width:26,height:26,borderRadius:8,display:'flex',alignItems:'center',justifyContent:'center',background:on?'linear-gradient(180deg,#2a2a2f,#18181b)':'transparent',border:'1px solid '+(on?'rgba(255,255,255,.08)':'transparent'),boxShadow:on?'inset 0 1px 0 rgba(255,255,255,.07)':'none'}}><Icon name={ic} size={15} color={on?'#ededed':'var(--text-muted)'}/></span><span style={{flex:1,whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{l}</span></button>})}
    </div>
    <div style={{padding:'28px 40px 48px',display:'flex',flexDirection:'column',gap:18,maxWidth:960,width:'100%',margin:'0 auto',boxSizing:'border-box'}}>
      
      {tab==='info'&&<Info p={p} set={setP} flash={flash}/>}
      {tab==='security'&&<window.ProfileSecurity flash={flash}/>}
      {tab==='activity'&&<Activity/>}
      {tab==='notif'&&<window.ProfileNotif/>}
    </div>
    {toast&&<div style={{position:'fixed',bottom:24,right:24,zIndex:60,padding:'10px 14px',borderRadius:'var(--radius-md)',background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)',font:'400 12px/1 var(--font-sans)',color:'var(--text-strong)',display:'flex',alignItems:'center',gap:8}}><Icon name="check" size={12} color="var(--green-500)"/>{toast}</div>}
  </div>;
}
window.ProfilePage=ProfilePage;
})();
