(()=>{
const {ButtonGroup,Select,GroupButton,Button,IconButton}=window.CipheraDesignSystem;
function DashboardHeader({title,period,setPeriod,range,actions}){
  return <header style={{display:'flex',alignItems:'center',justifyContent:'space-between',padding:'24px 30px 22px 32px'}}>
    <h1 style={{margin:0,font:'500 24px/1.2 var(--font-sans)',color:'var(--text-strong)',letterSpacing:'-0.01em',whiteSpace:'nowrap'}}>{title}</h1>
    {actions?<div style={{display:'flex',gap:8,alignItems:'center'}}>{actions}</div>:<div style={{display:'flex',gap:16,alignItems:'center'}}>
      <div style={{display:'flex',gap:0}}><ButtonGroup><Select bare options={Object.keys(window.CIPHERA.periods)} value={period} onChange={setPeriod}/><GroupButton icon="calendar">{range}</GroupButton></ButtonGroup></div>
      <div style={{display:'flex',gap:8}}><Button icon="sliders-horizontal">Personnaliser</Button><IconButton icon="ellipsis" label="Plus"/></div>
    </div>}
  </header>;
}
window.DashboardHeader=DashboardHeader;
})();
