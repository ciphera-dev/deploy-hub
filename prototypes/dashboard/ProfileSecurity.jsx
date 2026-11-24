(()=>{
const {Button,Icon,Switch}=window.CipheraDesignSystem;
const hair='1px solid var(--border-subtle)';
const card={borderRadius:16,border:'1px solid var(--border-default)',background:'var(--surface-app)',overflow:'hidden'};
const rgba=(h,a)=>{const n=parseInt(h.slice(1),16);return 'rgba('+(n>>16&255)+','+(n>>8&255)+','+(n&255)+','+a+')'};
const Tile=({ic,s=36})=><span style={{width:s,height:s,borderRadius:s*.28,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'linear-gradient(180deg,#232327,#141417)',border:'1px solid rgba(255,255,255,.08)',boxShadow:'inset 0 1px 0 rgba(255,255,255,.07), 0 1px 2px rgba(0,0,0,.4)'}}><Icon name={ic} size={s*.42} color="#ededed"/></span>;
const Pill=({ok,children})=><span style={{display:'inline-flex',alignItems:'center',gap:6,height:22,padding:'0 9px',borderRadius:99,background:ok?'rgba(34,192,138,.12)':'rgba(229,180,84,.12)',color:ok?'var(--green-500)':'#e5b454',font:'500 11px/1 var(--font-sans)',whiteSpace:'nowrap'}}><span style={{width:6,height:6,borderRadius:99,background:'currentColor'}}></span>{children}</span>;
const Title=({t,d,right})=><div style={{display:'flex',alignItems:'flex-end',gap:12,padding:'0 2px'}}><span style={{flex:1,display:'flex',flexDirection:'column',gap:6}}><span style={{font:'600 15px/1 var(--font-sans)',color:'var(--text-strong)'}}>{t}</span>{d&&<span style={{font:'400 12px/1.3 var(--font-sans)',color:'var(--text-subtle)'}}>{d}</span>}</span>{right}</div>;

function Ring({v}){const R=38,C=2*Math.PI*R;return <div style={{position:'relative',width:96,height:96,flex:'none'}}><svg width="96" height="96" style={{transform:'rotate(-90deg)'}}><defs><linearGradient id="pf-ring" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stopColor="#22c08a"/><stop offset="1" stopColor="#38bdf8"/></linearGradient></defs><circle cx="48" cy="48" r={R} fill="none" stroke="rgba(255,255,255,.07)" strokeWidth="8"/><circle cx="48" cy="48" r={R} fill="none" stroke="url(#pf-ring)" strokeWidth="8" strokeLinecap="round" strokeDasharray={C} strokeDashoffset={C*(1-v/100)} style={{transition:'stroke-dashoffset .6s cubic-bezier(.2,.8,.2,1)'}}/></svg><span style={{position:'absolute',inset:0,display:'flex',flexDirection:'column',alignItems:'center',justifyContent:'center',gap:4}}><span style={{font:'600 22px/1 var(--font-sans)',color:'var(--text-strong)'}}>{v}</span><span style={{font:'400 10px/1 var(--font-sans)',color:'var(--text-subtle)'}}>sur 100</span></span></div>}

function ProfileSecurity({flash}){
  const [totp,setTotp]=React.useState(true);const [wa,setWa]=React.useState(true);const [rec,setRec]=React.useState(false);
  const [sess,setSess]=React.useState([['laptop','MacBook Pro','Chrome 131','Genève, CH','185.12.4.20','Active maintenant',true],['smartphone','iPhone 15','Safari','Genève, CH','185.12.4.20','il y a 2 h'],['monitor','PC Windows','Firefox 132','Lyon, FR','92.184.10.3','il y a 3 j']]);
  const checks=[['Mot de passe robuste',true],['Application d\'authentification',totp],['Clé de sécurité',wa],['E-mail de récupération',rec]];
  const score=40+checks.filter(c=>c[1]).length*15;
  const M=[['key-round','#f59e0b','Mot de passe','Modifié il y a 2 mois',true,<Button size="sm" onClick={()=>flash('Lien de modification envoyé')}>Modifier</Button>],['smartphone','#8b6cff','Application d\'authentification','Google Authenticator, 1Password…',totp,<Switch checked={totp} onChange={v=>{setTotp(v);flash(v?'2FA activée':'2FA désactivée')}}/>],['fingerprint','#38bdf8','Clé de sécurité','YubiKey, Touch ID · '+(wa?'1 clé':'aucune'),wa,<Switch checked={wa} onChange={setWa}/>]];
  return <>
    <div style={{...card,display:'flex',flexWrap:'wrap',alignItems:'center',gap:24,padding:'22px 24px'}}>
      <Ring v={score}/>
      <div style={{display:'flex',flexDirection:'column',gap:8,flex:'1 1 180px',minWidth:0}}><span style={{font:'600 17px/1.2 var(--font-sans)',color:'var(--text-strong)'}}>{score>=90?'Compte très bien protégé':score>=70?'Bonne protection':'Protection à renforcer'}</span><span style={{font:'400 12px/1.45 var(--font-sans)',color:'var(--text-subtle)'}}>Activez toutes les méthodes pour atteindre 100.</span></div>
      <div style={{flex:'1 1 320px',minWidth:0,display:'grid',gridTemplateColumns:'repeat(auto-fill,minmax(200px,1fr))',gap:8}}>
        {checks.map(([t,ok])=><div key={t} style={{display:'flex',alignItems:'center',gap:10,height:40,padding:'0 12px',borderRadius:10,background:'var(--surface-raised)'}}>
          <span style={{width:18,height:18,borderRadius:99,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:ok?'var(--green-500)':'transparent',border:ok?'none':'1.5px dashed var(--gray-500)'}}>{ok&&<Icon name="check" size={11} color="#0b0b0b"/>}</span>
          <span style={{flex:1,font:'400 12px/1.2 var(--font-sans)',color:ok?'var(--text-body)':'var(--text-muted)'}}>{t}</span>
          {!ok&&t.startsWith('E-mail')&&<button onClick={()=>{setRec(true);flash('E-mail de récupération ajouté')}} style={{background:'none',border:0,padding:0,cursor:'pointer',color:'var(--text-strong)',font:'500 12px/1 var(--font-sans)',textDecoration:'underline',textUnderlineOffset:3}}>Ajouter</button>}
        </div>)}
      </div>
    </div>
    <Title t="Méthodes de connexion" d="Combinez-les pour sécuriser l'accès à la plateforme."/>
    <div style={{display:'grid',gridTemplateColumns:'repeat(auto-fill,minmax(220px,1fr))',gap:12}}>
      {M.map(([ic,c,t,d,ok,ctl])=><div key={t} style={{...card,padding:18,display:'flex',flexDirection:'column',gap:16}}>
        <div style={{display:'flex',alignItems:'flex-start',justifyContent:'space-between'}}><Tile ic={ic} c={c} s={40}/><Pill ok={ok}>{ok?'Active':'Inactive'}</Pill></div>
        <span style={{display:'flex',flexDirection:'column',gap:6}}><span style={{font:'500 14px/1.2 var(--font-sans)',color:'var(--text-strong)'}}>{t}</span><span style={{font:'400 12px/1.35 var(--font-sans)',color:'var(--text-subtle)'}}>{d}</span></span>
        <div style={{marginTop:'auto',display:'flex',justifyContent:'flex-end'}}>{ctl}</div>
      </div>)}
    </div>
    <Title t="Sessions actives" d={sess.length+' appareil'+(sess.length>1?'s':'')+' connecté'+(sess.length>1?'s':'')} right={sess.length>1&&<Button size="sm" icon="log-out" onClick={()=>{setSess(s=>s.filter(x=>x[6]));flash('Autres sessions déconnectées')}}>Déconnecter les autres</Button>}/>
    <div style={card}>{sess.map((x,i)=><div key={x[1]} className="pf-sess" style={{display:'grid',gridTemplateColumns:'44px minmax(0,1.3fr) minmax(0,1fr) 130px 40px',gap:14,alignItems:'center',padding:'14px 16px 14px 18px',borderTop:i?hair:'none'}}>
      <span style={{position:'relative',width:44,height:44,borderRadius:12,display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-raised)',border:'1px solid var(--border-default)'}}><Icon name={x[0]} size={18} color="var(--text-body)"/>{x[6]&&<span className="live-ping" style={{position:'absolute',right:-2,top:-2,width:10,height:10,borderRadius:99,background:'var(--green-500)',boxShadow:'0 0 0 2px var(--surface-app)'}}></span>}</span>
      <span style={{display:'flex',flexDirection:'column',gap:6,minWidth:0}}><span style={{display:'flex',alignItems:'center',gap:8,font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{x[1]}{x[6]&&<Pill ok>Cet appareil</Pill>}</span><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'}}>{x[2]}</span></span>
      <span style={{display:'flex',flexDirection:'column',gap:6,minWidth:0}}><span style={{display:'flex',alignItems:'center',gap:6,font:'400 13px/1 var(--font-sans)',color:'var(--text-body)'}}><Icon name="map-pin" size={12} color="var(--text-subtle)"/>{x[3]}</span><span style={{font:'400 11px/1 var(--font-mono)',color:'var(--text-subtle)'}}>{x[4]}</span></span>
      <span style={{font:'400 12px/1 var(--font-sans)',color:x[6]?'var(--green-500)':'var(--text-muted)'}}>{x[5]}</span>
      {!x[6]?<button title="Déconnecter" onClick={()=>{setSess(s=>s.filter(y=>y!==x));flash(x[1]+' déconnecté')}} style={{width:34,height:34,borderRadius:10,border:'1px solid rgba(239,68,68,.25)',background:'rgba(239,68,68,.06)',color:'#f87171',cursor:'pointer',display:'flex',alignItems:'center',justifyContent:'center'}}><Icon name="log-out" size={14}/></button>:<span></span>}
    </div>)}</div>
    <div style={{...card,display:'flex',alignItems:'center',gap:16,padding:'18px 20px',borderColor:'rgba(239,68,68,.22)'}}>
      <Tile ic="trash-2" c="#ef4444" s={40}/>
      <span style={{flex:1,display:'flex',flexDirection:'column',gap:6}}><span style={{font:'500 14px/1 var(--font-sans)',color:'var(--text-strong)'}}>Supprimer mon compte</span><span style={{font:'400 12px/1.4 var(--font-sans)',color:'var(--text-subtle)'}}>Définitif. Transférez d'abord la propriété de vos espaces.</span></span>
      <button onClick={()=>flash('Transférez la propriété avant de supprimer')} style={{height:34,padding:'0 14px',borderRadius:'var(--radius-md)',border:0,background:'#dc2626',color:'#fff',cursor:'pointer',font:'500 13px/1 var(--font-sans)'}}>Supprimer le compte</button>
    </div>
  </>;
}
window.ProfileSecurity=ProfileSecurity;window.ProfileUI={card,hair,rgba,Tile,Pill,Title};
})();
