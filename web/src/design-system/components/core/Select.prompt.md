Dropdown trigger + menu, e.g. the "Last 30 days" period picker.
```jsx
<Select options={['Last 7 days','Last 30 days','Last 90 days']} value={p} onChange={setP}/>
```
Use `bare` inside a ButtonGroup.

The menu renders in a fixed-position portal, so it is never clipped by modals or scroll containers; it flips above the trigger when there's no room below.
