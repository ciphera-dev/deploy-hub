(()=>{
const {Button,IconButton,Badge,Icon,StatusDot,Modal,Switch}=window.CipheraDesignSystem;
const lbl={font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'};
const line='1px solid rgba(255,255,255,.06)';
const CATS=['Tous','Code source','DNS et réseau','Sécurité','Bases de données','Notifications','IA'];
const CF_IMG='../../web/src/assets/images/cloudflare/cloudflare-mark.png';
const LIB=[
  {id:'github',n:'GitHub',cat:'Code source',img:'../../web/src/assets/images/connectors/github-mark.png',col:'#ededed',d:'Déploiement à chaque push et previews par pull request.',scopes:['Dépôts','Webhooks','Checks'],acct:'ciphera',on:true,sync:'il y a 3 min',ev:[4,6,3,8,12,9,7,11,14,10,8,13],evN:'1 204'},
  {id:'cloudflare',n:'Cloudflare',cat:'DNS et réseau',img:CF_IMG,col:'#f38020',d:'DNS automatique, proxy, WAF et certificats d\'origine.',scopes:['DNS','SSL','Zones'],acct:'Ciphera SAS',sync:'il y a 12 min',ev:[3,5,4,6,5,7,6,8,7,6,9,8],evN:'312'},
  {id:'crowdsec',n:'CrowdSec',cat:'Sécurité',key:'crowdsec',img:'../../web/src/assets/images/connectors/crowdsec-mark.png',col:'#8b7cf6',d:'Détection d\'intrusions et blocklists communautaires sur vos serveurs.',scopes:['Décisions','Alertes','Blocklists'],acct:'console · 4 agents',on:true,sync:'il y a 1 min',ev:[9,12,8,15,22,18,14,26,19,16,21,24],evN:'18 412'},
  {id:'supabase',n:'Supabase',cat:'Bases de données',img:'../../web/src/assets/images/connectors/supabase-mark.png',col:'#3ecf8e',d:'Postgres managé, auth et stockage. Variables injectées dans vos projets.',key:'supabase',scopes:['Projets','Base','Auth'],acct:'org ciphera'},
  {id:'smtp',n:'E-mail SMTP',cat:'Notifications',ic:'mail',col:'#7dd3fc',d:'Alertes, rapports et invitations via votre serveur SMTP.',key:'smtp',scopes:['Envoi'],acct:'smtp.ovh.net:587'},
  {id:'openai',n:'ChatGPT',wm:'../../web/src/assets/images/connectors/openai.png',org:'OpenAI',cat:'IA',img:'../../web/src/assets/images/connectors/chatgpt-mark.png',col:'#10a37f',d:'GPT pour l\'analyse des logs et les résumés d\'incidents.',key:'sk-...',scopes:['Chat','Embeddings'],acct:'org-ciphera · gpt-4.1',on:true,sync:'il y a 8 min',ev:[2,4,3,5,6,4,7,5,8,6,9,7],evN:'2 318'},
  {id:'claude',n:'Claude',wm:'../../web/src/assets/images/connectors/anthropic.png',org:'Anthropic',cat:'IA',img:'../../web/src/assets/images/connectors/claude-mark.png',col:'#d97757',d:'Modèles Anthropic pour l\'assistant et la revue de configuration.',key:'sk-ant-...',scopes:['Messages'],acct:'workspace ciphera'},
  {id:'groq',n:'Groq',wm:'../../web/src/assets/images/connectors/groq.png',org:'Groq',cat:'IA',img:'../../web/src/assets/images/connectors/groq-mark.png',col:'#f55036',d:'Inférence très rapide pour l\'autocomplétion et le tri des alertes.',key:'gsk_...',scopes:['Chat'],acct:'ciphera'},
  {id:'openrouter',n:'OpenRouter',org:'OpenRouter',cat:'IA',img:'../../web/src/assets/images/connectors/openrouter-mark.png',col:'#818cf8',d:'Un seul jeton pour des centaines de modèles, avec repli automatique.',key:'sk-or-...',scopes:['Chat','Routage'],acct:'ciphera'},
];
const rgb=h=>{const n=parseInt(h.slice(1),16);return (n>>16&255)+','+(n>>8&255)+','+(n&255)};

function Mark({c,size=40}){
  return <span style={{width:size,height:size,borderRadius:size*.28,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'linear-gradient(145deg,rgba('+rgb(c.col)+',.22),rgba('+rgb(c.col)+',.06))',border:'1px solid rgba('+rgb(c.col)+',.28)',boxShadow:'inset 0 1px 0 rgba(255,255,255,.08)'}}>
    {c.img?<img src={c.img} alt="" style={{width:size*.58,height:size*.58,objectFit:'contain'}}/>:<Icon name={c.ic} size={size*.46} color={c.col}/>}
  </span>;
}
function Spark({data,col}){const m=Math.max(...data,1);return <div style={{display:'flex',alignItems:'flex-end',gap:2,height:26}}>{data.map((v,i)=><span key={i} style={{flex:1,minWidth:3,height:Math.max(2,v/m*26),borderRadius:2,background:i===data.length-1?col:'rgba('+rgb(col)+',.35)'}}></span>)}</div>}
function Menu({items,onClose}){
  React.useEffect(()=>{const f=()=>onClose();setTimeout(()=>document.addEventListener('click',f),0);return()=>document.removeEventListener('click',f)},[]);
  return <div style={{position:'absolute',top:32,right:0,zIndex:30,width:210,padding:4,background:'var(--surface-tooltip)',borderRadius:'var(--radius-lg)',boxShadow:'var(--shadow-tooltip)'}}>
    {items.map((it,i)=>it==='-'?<div key={i} style={{height:1,background:'var(--border-strong)',margin:'4px 0'}}></div>:<button key={i} onClick={it[2]} onMouseEnter={e=>e.currentTarget.style.background='var(--surface-selected)'} onMouseLeave={e=>e.currentTarget.style.background='transparent'} style={{display:'flex',alignItems:'center',gap:10,width:'100%',padding:'8px 10px',border:0,borderRadius:6,background:'transparent',cursor:'pointer',textAlign:'left',color:it[3]?'#f87171':'var(--text-body)',font:'400 13px/1 var(--font-sans)'}}><Icon name={it[0]} size={14}/>{it[1]}</button>)}
  </div>;
}

const POP=new Set(['github','cloudflare','openai','claude']);
function Card({c,connected,onConnect,onConfig,onDisconnect,flash}){
  const [h,setH]=React.useState(false);const [menu,setMenu]=React.useState(false);const C=rgb(c.col);
  return <div onMouseEnter={()=>setH(true)} onMouseLeave={()=>setH(false)} style={{position:'relative',display:'flex',flexDirection:'column',borderRadius:16,overflow:'hidden',isolation:'isolate',border:'1px solid '+(h?'rgba('+C+',.32)':'rgba(255,255,255,.08)'),background:connected?'linear-gradient(115deg,#0f0f12 40%,rgba('+C+',.14))':'#0e0e11',transform:h?'translateY(-2px)':'none',boxShadow:h?'0 16px 36px -18px rgba('+C+',.55)':'none',transition:'all .25s'}}>
    <div style={{display:'flex',alignItems:'flex-start',gap:12,padding:'16px 16px 0'}}>
      <Mark c={c} size={40}/>
      <div style={{display:'flex',flexDirection:'column',gap:6,flex:1,minWidth:0,paddingTop:2}}>
        <span style={{font:'600 14px/1 var(--font-sans)',color:'var(--text-strong)'}}>{c.n}</span>
        <span style={{...lbl,fontSize:11}}>{c.org||c.cat}</span>
      </div>
      {connected?<div style={{position:'relative'}}><IconButton size={28} variant="ghost" icon="ellipsis" label="Actions" onClick={()=>setMenu(m=>!m)}/>{menu&&<Menu onClose={()=>setMenu(false)} items={[['settings-2','Configurer',onConfig],['refresh-cw','Réautoriser',()=>flash('Redirection vers '+c.n+'…')],'-',['unplug','Déconnecter',onDisconnect,true]]}/>}</div>
      :POP.has(c.id)&&<span style={{height:20,display:'flex',alignItems:'center',padding:'0 8px',borderRadius:99,background:'rgba(167,139,250,.12)',border:'1px solid rgba(167,139,250,.25)',font:'500 10px/1 var(--font-sans)',color:'#c4b5fd'}}>Populaire</span>}
    </div>
    <span style={{padding:'12px 16px 0',font:'400 12px/1.55 var(--font-sans)',color:'var(--text-muted)',textWrap:'pretty',minHeight:38}}>{c.d}</span>
    <div style={{display:'flex',flexDirection:'column',gap:8,padding:'14px 16px 16px'}}>
      <span style={{...lbl,fontSize:11}}>Fonctionnalités</span>
      <div style={{display:'flex',gap:5,flexWrap:'wrap'}}>{(c.scopes||['Accès']).map(x=><span key={x} style={{display:'flex',alignItems:'center',gap:5,height:22,padding:'0 8px',borderRadius:7,background:'rgba(255,255,255,.045)',border:'1px solid rgba(255,255,255,.06)',font:'500 11px/1 var(--font-sans)',color:'var(--text-body)'}}><Icon name="check" size={10} color={c.col}/>{x}</span>)}</div>
    </div>
    <div style={{display:'flex',alignItems:'center',gap:10,padding:'12px 16px',marginTop:'auto',borderTop:line}}>
      <span style={{display:'flex',flexDirection:'column',gap:6,flex:1,minWidth:0}}>
        <span style={{...lbl,fontSize:10}}>{connected?'Événements · 24 h':'Connexion'}</span>
        <span style={{display:'flex',alignItems:'center',gap:6,font:'600 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{connected?<><span style={{width:6,height:6,borderRadius:99,background:'#86efac',boxShadow:'0 0 6px rgba(134,239,172,.7)'}}></span>{c.evN}</>:c.key?'Clé d\'API':'OAuth'}</span>
      </span>
      {connected?<button onClick={onConfig} style={{height:34,padding:'0 14px',borderRadius:10,cursor:'pointer',font:'500 12px/1 var(--font-sans)',border:'1px solid rgba(255,255,255,.12)',background:'rgba(255,255,255,.04)',color:'var(--text-strong)'}}>Configurer</button>
      :<button onClick={onConnect} style={{height:34,padding:'0 16px',borderRadius:10,cursor:'pointer',font:'600 12px/1 var(--font-sans)',border:0,background:h?'#fff':'#ededed',color:'#0b0b0b',transition:'background .2s'}}>Connecter</button>}
    </div>
  </div>;
}
function ConnRow({c,first,onConfig,onDisconnect,flash}){
  const [h,setH]=React.useState(false);const [menu,setMenu]=React.useState(false);const [paused,setPaused]=React.useState(false);
  const m=Math.max(...c.ev,1);
  return <div onMouseEnter={()=>setH(true)} onMouseLeave={()=>setH(false)} style={{display:'grid',gridTemplateColumns:'minmax(0,1.5fr) minmax(0,1.2fr) 150px 110px 120px 108px',gap:20,alignItems:'center',padding:'14px 16px 14px 18px',borderTop:first?'none':'1px solid rgba(255,255,255,.06)',background:h?'rgba(255,255,255,.02)':'transparent',transition:'background .15s'}}>
    <div style={{display:'flex',alignItems:'center',gap:12,minWidth:0}}>
      <span style={{width:34,height:34,borderRadius:9,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'#16161a',border:'1px solid rgba(255,255,255,.08)'}}>{c.img?<img src={c.img} alt="" style={{width:18,height:18,objectFit:'contain'}}/>:<Icon name={c.ic} size={16} color="var(--text-strong)"/>}</span>
      <span style={{display:'flex',flexDirection:'column',gap:6,minWidth:0}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{c.n}</span><span style={{...lbl,fontSize:11}}>{c.cat}</span></span>
    </div>
    <span style={{display:'flex',alignItems:'center',gap:6,minWidth:0,font:'400 12px/1 var(--font-mono)',color:'var(--text-muted)'}}><span style={{overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'}}>{c.acct}</span></span>
    <div style={{display:'flex',alignItems:'flex-end',gap:2,height:20}}>{c.ev.map((v,i)=><span key={i} style={{flex:1,height:Math.max(2,v/m*20),borderRadius:1,background:i===c.ev.length-1?'#ededed':'rgba(255,255,255,.16)'}}></span>)}</div>
    <span style={{display:'flex',flexDirection:'column',gap:5}}><span style={{font:'500 13px/1 var(--font-mono)',color:'var(--text-strong)'}}>{c.evN}</span><span style={{...lbl,fontSize:10}}>événements · 24 h</span></span>
    <span style={{display:'flex',flexDirection:'column',gap:5}}><StatusDot status={paused?'off':'ok'} label={paused?'En pause':'Actif'}/><span style={{...lbl,fontSize:10}}>{c.sync}</span></span>
    <div style={{display:'flex',alignItems:'center',gap:4,justifyContent:'flex-end'}}>
      <Button size="sm" variant="ghost" onClick={onConfig}>Configurer</Button>
      <div style={{position:'relative'}}><IconButton size={28} variant="ghost" icon="ellipsis" label="Actions" onClick={()=>setMenu(x=>!x)}/>{menu&&<Menu onClose={()=>setMenu(false)} items={[[paused?'play':'pause',paused?'Reprendre':'Mettre en pause',()=>{setPaused(p=>!p);flash(c.n+(paused?' repris':' en pause'))}],['refresh-cw','Réautoriser',()=>flash('Redirection vers '+c.n+'…')],'-',['unplug','Déconnecter',onDisconnect,true]]}/>}</div>
    </div>
  </div>;
}
function PickerModal({items,onClose,onPick}){
  const [q,setQ]=React.useState('');const [cat,setCat]=React.useState('Tous');
  const cats=['Tous',...new Set(items.map(c=>c.cat))];
  const list=items.filter(c=>(cat==='Tous'||c.cat===cat)&&(c.n+c.d+(c.org||'')).toLowerCase().includes(q.toLowerCase()));
  return <Modal title="Ajouter un connecteur" description="Choisissez un service à relier à votre infrastructure." onClose={onClose} width={620}>
    <div style={{display:'flex',flexDirection:'column',gap:14}}>
      <div style={{display:'flex',alignItems:'center',gap:8,height:38,padding:'0 12px',borderRadius:10,border:'1px solid rgba(255,255,255,.1)',background:'#08080a'}}><Icon name="search" size={14} color="var(--text-subtle)"/><input autoFocus value={q} onChange={e=>setQ(e.target.value)} placeholder="Rechercher un service" style={{flex:1,minWidth:0,background:'transparent',border:0,outline:'none',color:'var(--text-strong)',font:'400 13px/1 var(--font-sans)'}}/></div>
      <div style={{display:'flex',gap:6,flexWrap:'wrap'}}>{cats.map(x=>{const on=cat===x;return <button key={x} onClick={()=>setCat(x)} style={{height:28,padding:'0 11px',borderRadius:99,cursor:'pointer',font:'500 12px/1 var(--font-sans)',border:'1px solid '+(on?'rgba(255,255,255,.25)':'rgba(255,255,255,.08)'),background:on?'rgba(255,255,255,.08)':'transparent',color:on?'var(--text-strong)':'var(--text-muted)'}}>{x}</button>})}</div>
      <div className="log-scroll" style={{display:'flex',flexDirection:'column',maxHeight:380,overflow:'auto',borderRadius:12,border:'1px solid rgba(255,255,255,.07)'}}>
        {list.map((c,i)=><PickRow key={c.id} c={c} first={i===0} onPick={()=>onPick(c)}/>)}
        {!list.length&&<div style={{padding:'28px 16px',textAlign:'center',...lbl}}>{items.length?'Aucun service ne correspond.':'Tous les services sont déjà connectés.'}</div>}
      </div>
    </div>
  </Modal>;
}
function PickRow({c,first,onPick}){
  const [h,setH]=React.useState(false);
  return <div onMouseEnter={()=>setH(true)} onMouseLeave={()=>setH(false)} style={{display:'flex',alignItems:'center',gap:14,padding:'12px 12px 12px 14px',borderTop:first?'none':'1px solid rgba(255,255,255,.06)',background:h?'rgba(255,255,255,.025)':'transparent',transition:'background .15s'}}>
    <span style={{width:36,height:36,borderRadius:9,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'#16161a',border:'1px solid rgba(255,255,255,.08)'}}>{c.img?<img src={c.img} alt="" style={{width:19,height:19,objectFit:'contain'}}/>:<Icon name={c.ic} size={16} color="var(--text-strong)"/>}</span>
    <span style={{display:'flex',flexDirection:'column',gap:6,flex:1,minWidth:0}}>
      <span style={{display:'flex',alignItems:'center',gap:8}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{c.n}</span><span style={{...lbl,fontSize:11}}>{c.org||c.cat}</span></span>
      <span style={{font:'400 12px/1.4 var(--font-sans)',color:'var(--text-muted)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{c.d}</span>
    </span>
    <span style={{...lbl,fontSize:11,whiteSpace:'nowrap'}}>{c.key==='supabase'?'URL + clé':c.key==='smtp'?'Serveur SMTP':c.key==='crowdsec'?'Clé API facultative':c.key?'Clé d\'API':'OAuth'}</span>
    <Button size="sm" variant={h?'inverse':'default'} onClick={onPick}>Connecter</Button>
  </div>;
}
function ConnectHero({c,step}){
  const C=rgb(c.col);const done=step===2,busy=step===1;
  const method=c.key==='smtp'?'SMTP · STARTTLS':c.key==='supabase'?'URL + clé':c.key?'Clé d\'API':'OAuth 2.0';
  const side=(child,name,sub,brand)=><div style={{display:'flex',flexDirection:'column',alignItems:'center',gap:10,width:104,flex:'none'}}>
    <span style={{width:60,height:60,borderRadius:17,display:'flex',alignItems:'center',justifyContent:'center',background:brand?'linear-gradient(160deg,rgba('+C+',.20),rgba('+C+',.05)) , #131316':'linear-gradient(160deg,#1d1d22,#121215)',border:'1px solid '+(brand?'rgba('+C+',.32)':'rgba(255,255,255,.12)'),boxShadow:'inset 0 1px 0 rgba(255,255,255,.08)'}}>{child}</span>
    <span style={{display:'flex',flexDirection:'column',alignItems:'center',gap:4}}><span style={{font:'600 12px/1 var(--font-sans)',color:'var(--text-strong)',whiteSpace:'nowrap'}}>{name}</span><span style={{font:'400 11px/1 var(--font-sans)',color:'var(--text-subtle)',whiteSpace:'nowrap'}}>{sub}</span></span>
  </div>;
  const pillCol=done?'134,239,172':'255,255,255';
  return <div style={{position:'relative',display:'flex',alignItems:'flex-start',justifyContent:'center',gap:0,padding:'24px 20px 20px',borderRadius:16,overflow:'hidden',isolation:'isolate',border:'1px solid rgba(255,255,255,.07)',background:'linear-gradient(90deg,rgba(255,255,255,.025),rgba('+C+',.07)),#0e0e11'}}>
    <span style={{position:'absolute',inset:0,zIndex:-1,backgroundImage:'linear-gradient(rgba(255,255,255,.035) 1px,transparent 1px),linear-gradient(90deg,rgba(255,255,255,.035) 1px,transparent 1px)',backgroundSize:'22px 22px',maskImage:'linear-gradient(180deg,#000,transparent 85%)',WebkitMaskImage:'linear-gradient(180deg,#000,transparent 85%)'}}></span>
    {side(<img src="../../web/src/assets/brand/ciphera-icon.svg" alt="Ciphera" style={{width:28,height:'auto',display:'block'}}/>,'Ciphera','Votre espace')}
    <div style={{position:'relative',flex:1,maxWidth:200,height:60,display:'flex',alignItems:'center',justifyContent:'center'}}>
      <span style={{position:'absolute',left:-6,right:-6,top:'50%',height:1,background:done?'rgba(134,239,172,.5)':'linear-gradient(90deg,rgba(255,255,255,.16),rgba('+C+',.45))'}}></span>
      <span style={{position:'absolute',left:-9,top:'50%',width:7,height:7,marginTop:-3.5,borderRadius:99,background:done?'#86efac':'#3a3a40'}}></span>
      <span style={{position:'absolute',right:-9,top:'50%',width:7,height:7,marginTop:-3.5,borderRadius:99,background:done?'#86efac':'rgb('+C+')'}}></span>
      <span style={{position:'relative',display:'flex',alignItems:'center',gap:6,height:26,padding:'0 10px',borderRadius:99,background:'#141417',border:'1px solid rgba('+pillCol+','+(done?'.45':'.12')+')',font:'500 11px/1 var(--font-sans)',color:done?'#86efac':'var(--text-body)',whiteSpace:'nowrap'}}><Icon name={done?'check':busy?'loader':'lock'} size={11} color={done?'#86efac':'var(--text-subtle)'}/>{done?'Connecté':busy?'Vérification…':method}</span>
    </div>
    {side(c.img?<img src={c.img} alt="" style={{width:28,height:28,objectFit:'contain'}}/>:<Icon name={c.ic} size={24} color={c.col}/>,c.n,c.org||c.cat,true)}
  </div>;
}
function ConnectModal({c,onClose,onDone}){
  const [step,setStep]=React.useState(0);const C=rgb(c.col);
  const [perm,setPerm]=React.useState({a:true,b:true,c:false});
  const PERMS={'SMTP':['Envoyer les alertes et incidents','Envoyer les rapports hebdomadaires','Envoyer les invitations d\'équipe'],'Bases de données':['Lire les projets et clés','Injecter DATABASE_URL et SUPABASE_*','Lancer les migrations'],'Sécurité':['Lire les alertes et décisions','Bannir et débannir des IP','Synchroniser les blocklists'],'IA':['Appeler les modèles de chat','Utiliser les embeddings','Suivre la consommation'],'Code source':['Lire les dépôts et branches','Créer des webhooks de déploiement','Publier le statut des builds'],'Registres':['Télécharger les images privées','Lister les dépôts d\'images','Publier des images'],'DNS et réseau':['Modifier les enregistrements DNS','Gérer les certificats SSL','Lire les statistiques de trafic'],'Stockage':['Écrire les sauvegardes','Lire et restaurer','Supprimer les anciennes sauvegardes'],'Notifications':['Publier dans les canaux choisis','Mentionner les membres d\'astreinte','Lire la liste des canaux'],'Observabilité':['Envoyer les métriques','Créer des releases','Lire les alertes']}[c.key==='smtp'?'SMTP':c.cat];
  const [k,setK]=React.useState('');const [url,setUrl]=React.useState('');const needKey=!!c.key;const isSb=c.key==='supabase';const isSmtp=c.key==='smtp';const isCs=c.key==='crowdsec';const [share,setShare]=React.useState(false);const [sm,setSm]=React.useState({host:'',port:'587',enc:'STARTTLS',user:'',pass:'',from:'',name:'Ciphera'});const [test,setTest]=React.useState(0);const smOk=/^[a-z0-9.-]+\.[a-z]{2,}$/i.test(sm.host.trim())&&/^\d{2,5}$/.test(sm.port)&&sm.user.trim()&&sm.pass;const urlOk=/^https:\/\/[a-z0-9-]+\.supabase\.(co|in)\/?$/i.test(url.trim());
  const go=()=>{setStep(1);setTimeout(()=>setStep(2),1500)};
  return <Modal onClose={onClose} width={520} footer={step===2?<Button variant="inverse" icon="check" onClick={()=>onDone(c,{key:k.trim(),share})}>Terminer</Button>:<><Button onClick={onClose}>Annuler</Button><Button variant="inverse" icon={step===1?'loader':needKey?'check':'external-link'} disabled={step===1||(needKey&&!isSmtp&&!isCs&&k.trim().length<6)||(isSb&&!urlOk)||(isSmtp&&!smOk)} onClick={go}>{step===1?(needKey?'Vérification…':'En attente de '+c.n+'…'):needKey?'Vérifier et connecter':'Autoriser sur '+c.n}</Button></>}>
    <div style={{display:'flex',flexDirection:'column',gap:20}}>
      <ConnectHero c={c} step={step}/>
      {step<2?<>
        {!isSmtp&&<div style={{display:'flex',flexDirection:'column',gap:6,textAlign:'center'}}><span style={{font:'600 17px/1.2 var(--font-sans)',color:'var(--text-strong)'}}>Connecter {c.n}</span><span style={{font:'400 13px/1.5 var(--font-sans)',color:'var(--text-muted)'}}>{c.d}</span></div>}
        {isSmtp&&(()=>{const set=(k2,v)=>setSm(x=>({...x,[k2]:v}));
          const PRE=[['Gmail','smtp.gmail.com','587','STARTTLS'],['Outlook','smtp.office365.com','587','STARTTLS'],['OVH','ssl0.ovh.net','587','STARTTLS'],['Brevo','smtp-relay.brevo.com','587','STARTTLS'],['Infomaniak','mail.infomaniak.com','587','STARTTLS'],['Hostinger','smtp.hostinger.com','587','STARTTLS'],['Autre','','587','STARTTLS']];
          const card=(ic,t,ok,children)=><div style={{display:'flex',flexDirection:'column',borderRadius:14,border:'1px solid rgba(255,255,255,.08)',background:'rgba(255,255,255,.02)'}}>
            <div style={{display:'flex',alignItems:'center',gap:8,padding:'12px 14px 10px'}}><Icon name={ic} size={14} color="var(--text-muted)"/><span style={{font:'600 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{t}</span><span style={{flex:1}}></span>{ok&&<Icon name="circle-check" size={14} color="#86efac"/>}</div>
            <div style={{display:'flex',flexDirection:'column',gap:10,padding:'0 14px 14px'}}>{children}</div></div>;
          const inp=(k2,ph,ty='text',extra)=><div style={{display:'flex',alignItems:'center',gap:8,height:44,padding:'0 14px',borderRadius:11,border:'1px solid '+(sm[k2]?'rgba(255,255,255,.18)':'rgba(255,255,255,.09)'),background:'rgba(255,255,255,.045)',minWidth:0,transition:'border-color .2s, background .2s'}}><input value={sm[k2]} onChange={e=>set(k2,e.target.value)} type={ty} placeholder={ph} style={{flex:1,minWidth:0,background:'transparent',border:0,outline:'none',color:'var(--text-strong)',font:'400 14px/1 var(--font-sans)'}}/>{extra}</div>;
          const hostOk=/^[a-z0-9.-]+\.[a-z]{2,}$/i.test(sm.host.trim())&&/^\d{2,5}$/.test(sm.port);const authOk=sm.user.trim()&&sm.pass;const fromOk=!!sm.name.trim();
          const pk=['a','b','c'],PL=['Alertes','Rapports','Invitations'];
          return <div style={{display:'flex',flexDirection:'column',gap:12}}>
            <div style={{display:'flex',alignItems:'center',gap:12,justifyContent:'center',textAlign:'center'}}><span style={{display:'flex',flexDirection:'column',gap:6,alignItems:'center'}}><span style={{font:'600 16px/1 var(--font-sans)',color:'var(--text-strong)'}}>Connecter un serveur SMTP</span><span style={{font:'400 12px/1.4 var(--font-sans)',color:'var(--text-muted)'}}>Pour les alertes, rapports et invitations.</span></span></div>
            <div style={{display:'flex',gap:6,flexWrap:'wrap'}}>{PRE.map(([n,h,p,e])=>{const on=n==='Autre'?!PRE.slice(0,-1).some(x=>x[1]===sm.host):sm.host===h;const lg={Gmail:'gmail',Outlook:'outlook',OVH:'ovh',Brevo:'brevo',Infomaniak:'infomaniak',Hostinger:'hostinger'}[n];return <button key={n} title={n} onClick={()=>setSm(x=>({...x,host:h,port:p,enc:e}))} style={{display:'flex',alignItems:'center',justifyContent:'center',height:38,padding:'0 16px',borderRadius:99,cursor:'pointer',font:'500 13px/1 var(--font-sans)',border:'1px solid '+(on?'rgba(255,255,255,.3)':'rgba(255,255,255,.08)'),background:on?'rgba(255,255,255,.08)':'transparent',color:on?'var(--text-strong)':'var(--text-muted)',transition:'all .15s',opacity:on?1:.7}}>{lg?<img src={'../../web/src/assets/images/smtp/'+lg+'.png'} alt={n} style={{height:lg==='outlook'||lg==='ovh'?15:lg==='gmail'?16:14,maxWidth:84,width:'auto',display:'block'}}/>:n}</button>})}</div>
            {card('server','Serveur',hostOk,<>
              {inp('host','smtp.exemple.com')}
              <div style={{display:'flex',gap:8}}>
                <div style={{flex:1,display:'grid',gridTemplateColumns:'repeat(3,1fr)',gap:3,padding:3,height:44,boxSizing:'border-box',borderRadius:11,border:'1px solid rgba(255,255,255,.09)',background:'rgba(255,255,255,.045)'}}>{[['STARTTLS','587',true],['Aucun','25',false],['SSL/TLS','465',false]].map(([e,p,ok])=>{const on=ok&&sm.enc===e;return <button key={e} disabled={!ok} title={ok?'':'Non disponible pour le moment'} onClick={()=>ok&&setSm(x=>({...x,enc:e,port:p}))} style={{display:'flex',alignItems:'center',justifyContent:'center',gap:5,borderRadius:8,border:0,cursor:ok?'pointer':'not-allowed',font:'500 12px/1 var(--font-sans)',background:on?'rgba(255,255,255,.12)':'transparent',color:on?'var(--text-strong)':ok?'var(--text-muted)':'var(--text-faint)',transition:'all .15s'}}>{e}{!ok&&<span title="Non disponible pour le moment" style={{display:'flex'}}><Icon name="info" size={11} color="var(--text-faint)"/></span>}</button>})}</div>
                <div style={{width:92}}>{inp('port','587')}</div>
              </div></>)}
            {card('key-round','Identifiants',authOk,<div style={{display:'grid',gridTemplateColumns:'1fr 1fr',gap:8}}>{inp('user','Utilisateur')}{inp('pass','Mot de passe','password')}</div>)}
            {card('send','Expéditeur',fromOk,<>
              {inp('name','Nom affiché')}
              <span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'}}>Apparaît comme <span style={{color:'var(--text-body)'}}>{(sm.name||'Ciphera')+' <'+(sm.user||'utilisateur')+'>'}</span></span>
            </>)}
            <div style={{display:'flex',alignItems:'center',gap:8,flexWrap:'wrap'}}>
              <span style={{font:'500 12px/1 var(--font-sans)',color:'var(--text-muted)',marginRight:2}}>Envoyer</span>
              {pk.map((x,i)=>{const on=perm[x];return <button key={x} onClick={()=>setPerm(p=>({...p,[x]:!p[x]}))} style={{display:'flex',alignItems:'center',gap:6,height:34,padding:'0 12px',borderRadius:99,cursor:'pointer',font:'500 13px/1 var(--font-sans)',border:'1px solid '+(on?'rgba(125,211,252,.35)':'rgba(255,255,255,.08)'),background:on?'rgba(125,211,252,.10)':'transparent',color:on?'var(--text-strong)':'var(--text-muted)',transition:'all .15s'}}><Icon name={on?'check':'plus'} size={13} color={on?'#7dd3fc':'var(--text-subtle)'}/>{PL[i]}</button>})}
              <span style={{flex:1}}></span>
              <button disabled={!smOk||test===1} onClick={()=>{setTest(1);setTimeout(()=>setTest(2),1200)}} style={{display:'flex',alignItems:'center',gap:6,height:34,padding:'0 12px',borderRadius:10,cursor:smOk?'pointer':'default',font:'500 12px/1 var(--font-sans)',border:'1px solid rgba(255,255,255,.1)',background:'transparent',color:smOk?(test===2?'#86efac':'var(--text-body)'):'var(--text-faint)'}}><Icon name={test===1?'loader':test===2?'check':'send'} size={13}/>{test===1?'Envoi…':test===2?'Test envoyé':'Tester'}</button>
            </div>
          </div>})()}
        {isSb&&<div style={{display:'flex',flexDirection:'column',gap:14}}>
          {[['URL du projet','globe',url,setUrl,'https://abcdefghijkl.supabase.co','text',url&&!urlOk?'Format attendu : https://<projet>.supabase.co':'Supabase › Project Settings › Data API'],['Clé service_role','key-round',k,setK,'eyJhbGciOiJIUzI1NiIsInR5cCI6…','password','Supabase › Project Settings › API Keys · gardée chiffrée côté serveur']].map(([t,ic,v,set,ph,ty,hint],j)=><div key={t} style={{display:'flex',flexDirection:'column',gap:8}}>
            <span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>{t}</span>
            <div style={{display:'flex',alignItems:'center',gap:8,height:38,padding:'0 12px',borderRadius:10,border:'1px solid '+(j===0&&url&&!urlOk?'rgba(248,113,113,.5)':'rgba('+C+','+(v?'.45':'.2')+')'),background:'#08080a',transition:'border-color .2s'}}><Icon name={ic} size={14} color={v?c.col:'var(--text-subtle)'}/><input autoFocus={j===0} value={v} onChange={e=>set(e.target.value)} type={ty} placeholder={ph} style={{flex:1,minWidth:0,background:'transparent',border:0,outline:'none',color:'var(--text-strong)',font:'400 13px/1 var(--font-mono)'}}/>{j===0&&urlOk&&<Icon name="circle-check" size={14} color="#86efac"/>}</div>
            <span style={{font:'400 11px/1.4 var(--font-sans)',color:j===0&&url&&!urlOk?'#f87171':'var(--text-faint)'}}>{hint}</span>
          </div>)}
        </div>}
        {isCs&&<div style={{display:'flex',flexDirection:'column',gap:10}}>
          <div style={{display:'flex',alignItems:'baseline',justifyContent:'space-between'}}><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>Clé API Console CrowdSec</span><span style={{font:'400 11px/1 var(--font-sans)',color:'var(--text-faint)'}}>Facultative</span></div>
          <div style={{display:'flex',alignItems:'center',gap:8,height:40,padding:'0 12px',borderRadius:10,border:'1px solid rgba(255,255,255,.1)',background:'rgba(255,255,255,.045)'}}><Icon name="key-round" size={14} color={k?c.col:'var(--text-subtle)'}/><input value={k} onChange={e=>setK(e.target.value)} type="password" placeholder="Laisser vide pour le plan gratuit" style={{flex:1,minWidth:0,background:'transparent',border:0,outline:'none',color:'var(--text-strong)',font:'400 13px/1 var(--font-mono)'}}/><span style={{height:20,display:'flex',alignItems:'center',padding:'0 7px',borderRadius:6,font:'700 10px/1 var(--font-sans)',letterSpacing:'.04em',background:k.trim().length>=6?'rgba(139,124,246,.18)':'rgba(255,255,255,.06)',color:k.trim().length>=6?'#c4b5fd':'var(--text-muted)'}}>{k.trim().length>=6?'PRO':'GRATUIT'}</span></div>
          <span style={{font:'400 11px/1.4 var(--font-sans)',color:'var(--text-faint)'}}>Avec une clé, Ciphera active le plan Pro : blocklists premium et historique étendu.</span>
          <label style={{display:'flex',alignItems:'flex-start',gap:12,padding:'12px 14px',borderRadius:12,border:'1px solid rgba(255,255,255,.08)',background:'rgba(255,255,255,.02)',cursor:'pointer'}}>
            <input type="checkbox" checked={share} onChange={e=>setShare(e.target.checked)} style={{marginTop:2,accentColor:'#8b7cf6',width:15,height:15}}/>
            <span style={{display:'flex',flexDirection:'column',gap:5}}><span style={{font:'500 13px/1.2 var(--font-sans)',color:'var(--text-strong)'}}>Partager les IP malveillantes avec la communauté</span><span style={{font:'400 12px/1.45 var(--font-sans)',color:'var(--text-muted)'}}>Les IP bannies sont envoyées anonymement à CrowdSec pour protéger les autres utilisateurs.</span></span>
          </label>
        </div>}
        {needKey&&!isSb&&!isSmtp&&!isCs&&<div style={{display:'flex',flexDirection:'column',gap:8}}><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>{c.key==='smtp'?'Serveur SMTP':'Clé d\'API'}</span><div style={{display:'flex',alignItems:'center',gap:8,height:38,padding:'0 12px',borderRadius:10,border:'1px solid rgba('+C+','+(k?'.45':'.2')+')',background:'#08080a',transition:'border-color .2s'}}><Icon name={c.key==='smtp'?'server':'key-round'} size={14} color={k?c.col:'var(--text-subtle)'}/><input autoFocus value={k} onChange={e=>setK(e.target.value)} type={c.key==='smtp'?'text':'password'} placeholder={c.key==='smtp'?'utilisateur:motdepasse@smtp.exemple.com:587':c.key} style={{flex:1,minWidth:0,background:'transparent',border:0,outline:'none',color:'var(--text-strong)',font:'400 13px/1 var(--font-mono)'}}/></div></div>}
        <div style={{borderRadius:12,border:line,overflow:'hidden',display:isSmtp?'none':'block'}}>
          {PERMS.map((p,i)=>{const k='abc'[i];return <div key={p} style={{display:'flex',alignItems:'center',gap:12,padding:'12px 14px',borderTop:i?line:'none'}}><Icon name={perm[k]?'circle-check':'circle'} size={15} color={perm[k]?c.col:'var(--text-faint)'}/><span style={{flex:1,font:'400 13px/1 var(--font-sans)',color:'var(--text-body)'}}>{p}</span><Switch checked={perm[k]} onChange={v=>setPerm(x=>({...x,[k]:v}))}/></div>})}
        </div>
        <span style={{display:'flex',alignItems:'center',gap:8,justifyContent:'center',...lbl,fontSize:11}}><Icon name="lock" size={11}/>Jetons chiffrés, jamais renvoyés par l'API. Révocable à tout moment.</span>
      </>:<div style={{display:'flex',flexDirection:'column',alignItems:'center',gap:8,textAlign:'center',padding:'4px 0 6px'}}>
        <span style={{width:40,height:40,borderRadius:99,display:'flex',alignItems:'center',justifyContent:'center',background:'rgba(134,239,172,.12)',border:'1px solid rgba(134,239,172,.3)'}}><Icon name="check" size={18} color="#86efac"/></span>
        <span style={{font:'600 16px/1.2 var(--font-sans)',color:'var(--text-strong)'}}>{c.n} est connecté</span>
        <span style={{font:'400 13px/1.5 var(--font-sans)',color:'var(--text-muted)'}}>La première synchronisation démarre maintenant.</span>
      </div>}
    </div>
  </Modal>;
}

function ConfigDrawer({c,onClose,onDisconnect,flash}){
  const C=rgb(c.col);const [n,setN]=React.useState({deploy:true,fail:true,pr:c.cat==='Code source'});
  const LOG=[['ok','Synchronisation terminée',c.sync],['ok','Webhook reçu · push main','il y a 14 min'],['ok','Jeton rafraîchi','il y a 2 h'],['error','Délai dépassé (retenté avec succès)','hier']];
  return <div style={{position:'fixed',inset:0,zIndex:70,display:'flex',justifyContent:'flex-end',background:'rgba(0,0,0,.5)',backdropFilter:'blur(2px)'}} onClick={onClose}>
    <div onClick={e=>e.stopPropagation()} style={{width:440,height:'100%',display:'flex',flexDirection:'column',background:'#0c0c0f',borderLeft:'1px solid rgba(255,255,255,.08)',boxShadow:'-24px 0 60px rgba(0,0,0,.5)',animation:'drawerIn .3s cubic-bezier(.2,.8,.2,1)'}}>
      <div style={{position:'relative',padding:'22px 20px 18px',background:'radial-gradient(120% 100% at 0% 0%,rgba('+C+',.18),transparent 60%)',borderBottom:line}}>
        <div style={{display:'flex',alignItems:'center',gap:14}}><Mark c={c} size={48}/><span style={{display:'flex',flexDirection:'column',gap:7,flex:1}}><span style={{font:'600 17px/1 var(--font-sans)',color:'var(--text-strong)'}}>{c.n}</span><StatusDot status="ok" label={'Connecté · '+c.acct}/></span><IconButton size={30} variant="ghost" icon="x" label="Fermer" onClick={onClose}/></div>
      </div>
      <div style={{flex:1,overflow:'auto',padding:20,display:'flex',flexDirection:'column',gap:22}} className="log-scroll">
        <section style={{display:'flex',flexDirection:'column',gap:10}}><span style={{font:'500 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>Permissions accordées</span><div style={{display:'flex',gap:6,flexWrap:'wrap'}}>{c.scopes.map(s=><span key={s} style={{display:'flex',alignItems:'center',gap:6,height:26,padding:'0 10px',borderRadius:8,background:'rgba('+C+',.08)',border:'1px solid rgba('+C+',.22)',font:'500 12px/1 var(--font-sans)',color:'var(--text-strong)'}}><Icon name="check" size={11} color={c.col}/>{s}</span>)}</div></section>
        <section style={{display:'flex',flexDirection:'column',gap:10}}><span style={{font:'500 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>Déclencheurs</span><div style={{borderRadius:12,border:line}}>{[['deploy','Déploiement réussi'],['fail','Échec de build'],['pr','Preview de pull request']].map(([k,t],i)=><div key={k} style={{padding:'12px 14px',borderTop:i?line:'none'}}><Switch checked={n[k]} onChange={v=>setN(x=>({...x,[k]:v}))} label={t}/></div>)}</div></section>
        <section style={{display:'flex',flexDirection:'column',gap:10}}><span style={{font:'500 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>Activité récente</span>
          <div style={{position:'relative',display:'flex',flexDirection:'column'}}><span style={{position:'absolute',left:5,top:8,bottom:8,width:1,background:'rgba(255,255,255,.08)'}}></span>
            {LOG.map(([s,t,w],i)=><div key={i} style={{display:'flex',alignItems:'center',gap:12,padding:'8px 0'}}><span style={{width:11,height:11,borderRadius:99,zIndex:1,background:'#0c0c0f',border:'2px solid '+(s==='ok'?'rgba(134,239,172,.6)':'rgba(248,113,113,.7)')}}></span><span style={{flex:1,font:'400 12px/1.3 var(--font-sans)',color:'var(--text-body)'}}>{t}</span><span style={{...lbl,fontSize:11}}>{w}</span></div>)}
          </div></section>
      </div>
      <div style={{display:'flex',gap:8,padding:'14px 20px',borderTop:line}}><Button icon="unplug" variant="ghost" onClick={onDisconnect}>Déconnecter</Button><span style={{flex:1}}></span><Button variant="inverse" onClick={()=>{flash('Configuration de '+c.n+' enregistrée');onClose()}}>Enregistrer</Button></div>
    </div>
  </div>;
}

function ConnectorsPage({addOpen,closeAdd}){
  const [cf,setCf]=window.useCloudflare?window.useCloudflare():[null,()=>{}];
  const [on,setOn]=React.useState(()=>{try{const v=JSON.parse(localStorage.getItem('ciphera.connectors.on'));if(Array.isArray(v))return new Set(v)}catch(e){}return new Set(LIB.filter(c=>c.on).map(c=>c.id))});
  React.useEffect(()=>{try{localStorage.setItem('ciphera.connectors.on',JSON.stringify([...on]))}catch(e){}window.dispatchEvent(new Event('ciphera:connectors'))},[on]);
  const [cat,setCat]=React.useState('Tous');const [q,setQ]=React.useState('');const [conn,setConn]=React.useState(null);const [cfg,setCfg]=React.useState(null);const [confirm,setConfirm]=React.useState(null);const [pick,setPick]=React.useState(false);
  const [toast,setToast]=React.useState('');const flash=m=>{setToast(m);clearTimeout(flash.t);flash.t=setTimeout(()=>setToast(''),2200)};
  const isOn=c=>c.id==='cloudflare'?!!cf:on.has(c.id);
  const full=c=>({scopes:['Accès'],acct:'ciphera',sync:"à l'instant",ev:[0,0,0,0,0,0,0,0,0,0,0,1],evN:'1',...c});
  const match=c=>(cat==='Tous'||c.cat===cat)&&(c.n+c.d).toLowerCase().includes(q.toLowerCase());
  const connected=LIB.filter(c=>isOn(c)&&match(c)).map(full),avail=LIB.filter(c=>!isOn(c)&&match(c));
  const nOn=LIB.filter(isOn).length;
  const done=(c,o={})=>{if(c.id==='crowdsec'){try{localStorage.setItem('ciphera.crowdsec',JSON.stringify({plan:o.key&&o.key.length>=6?'pro':'free',share:!!o.share}))}catch(e){}window.dispatchEvent(new Event('ciphera:connectors'))}if(c.id==='cloudflare')setCf({account:'Ciphera SAS',email:'alex@example.com',zones:['ciphera.com','example.com'],via:'oauth',at:"à l'instant",proxy:true});else setOn(s=>new Set([...s,c.id]));setConn(null);flash(c.n+' connecté')};
  const disconnect=c=>{if(c.id==='cloudflare')setCf(null);else setOn(s=>{const n=new Set(s);n.delete(c.id);return n});setConfirm(null);setCfg(null);flash(c.n+' déconnecté')};
  React.useEffect(()=>{if(addOpen){setPick(true);closeAdd&&closeAdd()}},[addOpen]);
  const box={borderRadius:20,border:'1px solid rgba(255,255,255,.08)',background:'#0c0c0f'};
  const gh=LIB.find(x=>x.id==='github'),cl=LIB.find(x=>x.id==='claude'),cfl=LIB.find(x=>x.id==='cloudflare');
  const STEPS=[['layout-grid','Choisissez un service','Code source, DNS, sécurité, base de données ou IA : chaque connecteur s\'active en un clic.'],['key-round','Autorisez l\'accès','Connexion OAuth ou clé d\'API. Vous choisissez les permissions, révocables à tout moment.'],['zap','Laissez Ciphera travailler','Déploiements, DNS, bannissements et analyses de logs se synchronisent automatiquement.']];
  return <div style={{padding:'20px 32px 40px 34px',display:'flex',flexDirection:'column',gap:16}}>
    <div style={{...box,display:'grid',gridTemplateColumns:'minmax(0,1.1fr) minmax(0,1fr)',gap:32,alignItems:'center',padding:'32px 32px 32px 36px',position:'relative',overflow:'hidden',isolation:'isolate'}}>
      <span style={{position:'absolute',inset:0,zIndex:-1,background:'radial-gradient(60% 90% at 100% 0%,rgba(139,92,246,.10),transparent 70%)'}}></span>
      <div style={{display:'flex',flexDirection:'column',gap:16,alignItems:'flex-start'}}>
        <span style={{display:'flex',alignItems:'center',gap:6,height:24,padding:'0 10px',borderRadius:99,background:'rgba(167,139,250,.10)',border:'1px solid rgba(167,139,250,.22)',font:'500 11px/1 var(--font-sans)',color:'#c4b5fd'}}><Icon name="plug-zap" size={12}/>{nOn} connectés sur {LIB.length}</span>
        <span style={{font:'600 30px/1.15 var(--font-sans)',color:'var(--text-strong)',letterSpacing:'-0.02em',textWrap:'balance'}}>Connectez vos outils à votre infrastructure</span>
        <span style={{font:'400 14px/1.6 var(--font-sans)',color:'var(--text-muted)',maxWidth:480,textWrap:'pretty'}}>Reliez vos dépôts, votre DNS, votre sécurité et vos modèles d'IA. Ciphera synchronise tout en arrière-plan, sans configuration manuelle.</span>
        <div style={{display:'flex',gap:8,marginTop:4}}>
          <button onClick={()=>setPick(true)} style={{display:'flex',alignItems:'center',gap:8,height:38,padding:'0 16px',borderRadius:11,cursor:'pointer',border:0,background:'#ededed',color:'#0b0b0b',font:'600 13px/1 var(--font-sans)',boxShadow:'0 0 0 1px rgba(167,139,250,.5), 0 6px 18px -6px rgba(139,92,246,.6)'}}><Icon name="plus" size={14}/>Ajouter un connecteur</button>
          <Button variant="ghost" iconRight="arrow-up-right">Documentation</Button>
        </div>
      </div>
      <div style={{position:'relative',padding:'28px 28px 44px',borderRadius:16,background:'rgba(255,255,255,.025)',border:'1px solid rgba(255,255,255,.05)'}}>
        <div style={{borderRadius:12,border:'1px solid rgba(255,255,255,.09)',background:'#111115',overflow:'hidden'}}>
          <div style={{display:'flex',alignItems:'center',gap:8,padding:'10px 12px',borderBottom:line,font:'500 12px/1 var(--font-mono)',color:'var(--text-body)'}}><Icon name="layers" size={12} color="var(--text-subtle)"/>ciphera-web<span style={{flex:1}}></span><span style={{display:'flex',alignItems:'center',gap:5,...lbl,fontSize:11}}><Icon name="star" size={11}/>Production</span></div>
          <div style={{display:'flex',flexDirection:'column',gap:10,padding:'14px 12px'}}>
            {[gh,cfl,cl].map((c,i)=><div key={c.id} style={{display:'flex',alignItems:'center',gap:10}}><Mark c={c} size={26}/><span style={{flex:1,font:'500 12px/1 var(--font-sans)',color:'var(--text-strong)'}}>{c.n}</span><span style={{height:5,width:[70,52,38][i],borderRadius:99,background:'rgba(255,255,255,.07)'}}></span><Icon name="circle-check" size={13} color="#86efac"/></div>)}
          </div>
        </div>
        <div style={{position:'absolute',left:44,right:16,bottom:14,display:'flex',alignItems:'center',gap:10,padding:'10px 12px',borderRadius:11,background:'#18181c',border:'1px solid rgba(255,255,255,.1)',boxShadow:'0 14px 30px -10px rgba(0,0,0,.7)'}}>
          <span style={{width:18,height:18,borderRadius:99,display:'flex',alignItems:'center',justifyContent:'center',background:'rgba(134,239,172,.15)'}}><Icon name="check" size={11} color="#86efac"/></span>
          <span style={{display:'flex',flexDirection:'column',gap:4}}><span style={{font:'600 12px/1 var(--font-sans)',color:'var(--text-strong)'}}>Webhook GitHub reçu</span><span style={{...lbl,fontSize:11}}>Déploiement de main lancé automatiquement</span></span>
        </div>
      </div>
    </div>
    <div style={{...box,padding:'26px 28px 28px',display:'flex',flexDirection:'column',gap:18}}>
      <div style={{display:'flex',flexDirection:'column',gap:6}}><span style={{font:'600 16px/1 var(--font-sans)',color:'var(--text-strong)'}}>Comment ça marche</span><span style={{font:'400 13px/1 var(--font-sans)',color:'var(--text-muted)'}}>Choisissez, autorisez, c'est synchronisé.</span></div>
      <div style={{display:'grid',gridTemplateColumns:'repeat(3,minmax(0,1fr))',gap:12}}>
        {STEPS.map(([ic,t,d],i)=><div key={t} style={{display:'flex',flexDirection:'column',gap:14,padding:18,borderRadius:14,background:'rgba(255,255,255,.03)',border:'1px solid rgba(255,255,255,.05)'}}>
          <div style={{display:'flex',alignItems:'center',justifyContent:'space-between'}}><span style={{width:38,height:38,borderRadius:10,display:'flex',alignItems:'center',justifyContent:'center',background:'#16161a',border:'1px solid rgba(255,255,255,.1)'}}><Icon name={ic} size={16} color="var(--text-strong)"/></span><span style={{...lbl,fontSize:11}}>Étape {i+1}</span></div>
          <span style={{font:'600 14px/1.2 var(--font-sans)',color:'var(--text-strong)'}}>{t}</span>
          <span style={{font:'400 12px/1.55 var(--font-sans)',color:'var(--text-muted)',textWrap:'pretty'}}>{d}</span>
        </div>)}
      </div>
      <div style={{display:'flex',gap:12,padding:'14px 16px',borderRadius:12,background:'rgba(139,92,246,.08)',border:'1px solid rgba(139,92,246,.18)'}}>
        <Icon name="shield-check" size={16} color="#c4b5fd"/>
        <span style={{display:'flex',flexDirection:'column',gap:5}}><span style={{font:'600 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>Vos identifiants restent chiffrés.</span><span style={{font:'400 12px/1.5 var(--font-sans)',color:'var(--text-muted)'}}>Jetons et clés d'API sont stockés en AES-256, jamais renvoyés par l'API, et peuvent être révoqués à tout moment.</span></span>
      </div>
    </div>
    <div id="conn-cat" style={{display:'flex',alignItems:'center',gap:10,flexWrap:'wrap',marginTop:8}}>
      <div style={{display:'flex',gap:2,padding:3,borderRadius:11,border:'1px solid rgba(255,255,255,.07)',background:'rgba(255,255,255,.02)',flexWrap:'wrap'}}>
        {CATS.map(x=>{const on2=cat===x;const n=x==='Tous'?LIB.length:LIB.filter(c=>c.cat===x).length;return <button key={x} onClick={()=>setCat(x)} style={{display:'flex',alignItems:'center',gap:6,height:30,padding:'0 11px',border:0,borderRadius:8,cursor:'pointer',font:'500 12px/1 var(--font-sans)',background:on2?'rgba(255,255,255,.09)':'transparent',color:on2?'var(--text-strong)':'var(--text-muted)',transition:'all .15s'}}>{x}<span style={{font:'500 10px/1 var(--font-mono)',color:on2?'var(--text-muted)':'var(--text-faint)'}}>{n}</span></button>})}
      </div>
      <span style={{flex:1}}></span>
      <div style={{display:'flex',alignItems:'center',gap:8,height:34,padding:'0 12px',width:240,borderRadius:10,border:'1px solid rgba(255,255,255,.08)',background:'rgba(255,255,255,.02)'}}><Icon name="search" size={14} color="var(--text-subtle)"/><input value={q} onChange={e=>setQ(e.target.value)} placeholder="Rechercher un service" style={{flex:1,minWidth:0,background:'transparent',border:0,outline:'none',color:'var(--text-strong)',font:'400 13px/1 var(--font-sans)'}}/></div>
    </div>
    {connected.length>0&&<section style={{display:'flex',flexDirection:'column',gap:12}}>
      <span style={{display:'flex',alignItems:'center',gap:8,font:'600 14px/1 var(--font-sans)',color:'var(--text-strong)'}}>Connectés<span style={{...lbl,fontFamily:'var(--font-mono)'}}>{connected.length}</span></span>
      <div style={{borderRadius:14,border:'1px solid rgba(255,255,255,.08)',background:'#0c0c0f',overflow:'hidden'}}>
        <div style={{display:'grid',gridTemplateColumns:'minmax(0,1.5fr) minmax(0,1.2fr) 150px 110px 120px 108px',gap:20,padding:'10px 16px 10px 18px',borderBottom:'1px solid rgba(255,255,255,.06)',...lbl,fontSize:11}}><span>Service</span><span>Compte</span><span>Activité</span><span>Volume</span><span>Statut</span><span></span></div>
        {connected.map((c,i)=><ConnRow key={c.id} c={c} first={i===0} flash={flash} onConfig={()=>setCfg(c)} onDisconnect={()=>setConfirm(c)}/>)}
      </div>
    </section>}
    {!connected.length&&<div style={{padding:'40px 0',textAlign:'center',...lbl}}>Aucun connecteur actif.</div>}
    {pick&&<PickerModal items={LIB.filter(c=>!isOn(c))} onClose={()=>setPick(false)} onPick={c=>{setPick(false);setConn(c)}}/>}
    {conn&&<ConnectModal c={conn} onClose={()=>setConn(null)} onDone={done}/>}
    {cfg&&<ConfigDrawer c={cfg} flash={flash} onClose={()=>setCfg(null)} onDisconnect={()=>setConfirm(cfg)}/>}
    {confirm&&<Modal title={'Déconnecter '+confirm.n+' ?'} description="Les jetons sont révoqués et les synchronisations s'arrêtent. Vos projets restent en ligne." onClose={()=>setConfirm(null)} width={440} footer={<><Button onClick={()=>setConfirm(null)}>Annuler</Button><Button variant="inverse" icon="unplug" onClick={()=>disconnect(confirm)}>Déconnecter</Button></>}></Modal>}
    {toast&&<div style={{position:'fixed',bottom:24,right:24,zIndex:80,padding:'10px 14px',borderRadius:'var(--radius-md)',background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)',font:'400 12px/1 var(--font-sans)',color:'var(--text-strong)',display:'flex',alignItems:'center',gap:8}}><Icon name="check" size={12} color="var(--green-500)"/>{toast}</div>}
  </div>;
}
window.ConnectorsPageV2=ConnectorsPage;window.CIPHERA_LLM=LIB.filter(x=>x.cat==='IA');
})();
