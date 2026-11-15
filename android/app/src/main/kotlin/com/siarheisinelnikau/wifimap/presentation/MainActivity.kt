package com.siarheisinelnikau.wifimap.presentation

import android.Manifest
import android.app.AlertDialog
import android.content.Context
import android.content.Intent
import android.graphics.Color
import android.graphics.drawable.GradientDrawable
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import android.text.InputType
import android.view.*
import android.widget.*
import androidx.activity.ComponentActivity
import androidx.activity.addCallback
import androidx.activity.result.contract.ActivityResultContracts
import androidx.lifecycle.*
import com.siarheisinelnikau.wifimap.BuildConfig
import com.siarheisinelnikau.wifimap.data.*
import com.siarheisinelnikau.wifimap.domain.*
import com.siarheisinelnikau.wifimap.platform.AndroidWifiScanner
import kotlinx.coroutines.launch
import java.time.Instant

class MainActivity : ComponentActivity() {
    private lateinit var model: SurveyViewModel
    private lateinit var text: Text
    private lateinit var palette: Palette
    private lateinit var canvas: PlanCanvas
    private lateinit var root: LinearLayout
    private lateinit var heading: TextView; private lateinit var status: TextView; private lateinit var selected: TextView; private lateinit var legend: TextView
    private lateinit var survey: Spinner; private lateinit var ap: Spinner; private lateinit var compare: CheckBox
    private lateinit var measure: Button; private lateinit var cancel: Button; private lateinit var settingsButton: Button
    private val editButtons = ArrayList<View>()
    private val toolButtons = linkedMapOf<Tool, Button>()
    private var rendering = false; private var appearance: UserPreferences? = null
    private var settingsDialog: AlertDialog? = null; private var settingsBody: LinearLayout? = null; private var settingsKey: Pair<UserPreferences, UpdateStatus>? = null
    private val projectPicker = registerForActivityResult(ActivityResultContracts.OpenDocument()) { result(it) }
    private val imagePicker = registerForActivityResult(ActivityResultContracts.OpenDocument()) { result(it) }
    private val projectCreator = registerForActivityResult(ActivityResultContracts.CreateDocument("application/json")) { result(it) }
    private val reportCreator = registerForActivityResult(ActivityResultContracts.CreateDocument("image/svg+xml")) { result(it) }
    private val noticesCreator = registerForActivityResult(ActivityResultContracts.CreateDocument("text/plain")) { result(it) }
    private val permissions = registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { model.permissionResult(model.scanner.permissionGranted()) }
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        model = ViewModelProvider(this, object : ViewModelProvider.Factory {
            @Suppress("UNCHECKED_CAST") override fun <T : ViewModel> create(modelClass: Class<T>): T = SurveyViewModel(ProjectStore(applicationContext), AndroidWifiScanner(applicationContext), Preferences(applicationContext), AndroidUpdater(applicationContext)) as T
        })[SurveyViewModel::class.java]
        onBackPressedDispatcher.addCallback(this) { model.navigate(Navigation.Close) }
        lifecycleScope.launch { repeatOnLifecycle(Lifecycle.State.STARTED) {
            launch { model.state.collect(::render) }
            launch { model.effects.collect(::effect) }
        } }
    }
    override fun onStart() { super.onStart(); if (::model.isInitialized) model.onForeground() }
    override fun onStop() { if (::canvas.isInitialized) canvas.stopMotion(); if (::model.isInitialized) { if (isFinishing) model.cancel() else model.onBackground() }; super.onStop() }
    override fun onResume() {
        super.onResume()
        val result = getSharedPreferences("wifimap-update-result", MODE_PRIVATE)
        result.getString("result", null)?.let { key -> model.installationResult(key, result.getString("details", "").orEmpty()); result.edit().clear().apply() }
    }
    private fun dp(value: Int) = (value * resources.displayMetrics.density).toInt()
    private fun column(context: Context = this) = LinearLayout(context).also { it.orientation = LinearLayout.VERTICAL }
    private fun row(context: Context = this) = LinearLayout(context).also { it.orientation = LinearLayout.HORIZONTAL; it.gravity = Gravity.CENTER_VERTICAL }
    private fun label(value: String = "", size: Float = 14f) = TextView(this).also { it.text = value; it.textSize = size; it.setTextColor(palette.text); it.setPadding(dp(8), dp(4), dp(8), dp(4)) }
    private fun border(fill: Int) = GradientDrawable().also { it.setColor(fill); it.cornerRadius = dp(8).toFloat(); it.setStroke(dp(1), palette.border) }
    private fun button(key: String, primary: Boolean = false, edit: Boolean = true, action: () -> Unit): Button = Button(this).also {
        it.text = text[key]; it.isAllCaps = false; it.textSize = 14f; it.minimumHeight = dp(48); it.minHeight = dp(48)
        it.setTextColor(if (primary) Color.WHITE else palette.text); it.background = border(if (primary) 0xff0b736f.toInt() else palette.surface)
        it.setPadding(dp(12), dp(4), dp(12), dp(4)); it.setOnClickListener { action() }; it.tooltipText = text[key]
        if (edit) editButtons.add(it)
    }
    private fun scrollRow(parent: LinearLayout, content: LinearLayout) { parent.addView(HorizontalScrollView(this).also { it.isHorizontalScrollBarEnabled = false; it.addView(content) }, LinearLayout.LayoutParams(-1, dp(52))) }
    private fun rebuild(state: SurveyState) {
        val reopen = settingsDialog?.isShowing == true; settingsDialog?.dismiss(); settingsDialog = null; settingsBody = null; settingsKey = null
        text = Text(this, state.preferences.language); palette = Palette.of(state.preferences.dark); appearance = state.preferences
        editButtons.clear(); toolButtons.clear(); root = column().also { it.setBackgroundColor(palette.background) }
        @Suppress("DEPRECATION")
        window.decorView.systemUiVisibility = if (state.preferences.dark) 0 else View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR
        root.setOnApplyWindowInsetsListener { view, insets ->
            @Suppress("DEPRECATION") val bars = if (Build.VERSION.SDK_INT >= 30) insets.getInsets(WindowInsets.Type.systemBars()).let { intArrayOf(it.left,it.top,it.right,it.bottom) } else intArrayOf(insets.systemWindowInsetLeft,insets.systemWindowInsetTop,insets.systemWindowInsetRight,insets.systemWindowInsetBottom)
            view.setPadding(dp(8)+bars[0],bars[1],dp(8)+bars[2],bars[3]); insets
        }
        heading = label("WifiMap", 22f).also { it.setTypeface(null, android.graphics.Typeface.BOLD); it.contentDescription = text["plan_name"]; it.setOnClickListener { if (model.state.value.busy == null) nameDialog() } }; root.addView(heading)
        val actions = row()
        actions.addView(button("open") { model.navigate(Navigation.Open) }); actions.addView(button("import") { model.navigate(Navigation.Import) }); actions.addView(button("save") { model.pick(FileAction.Save) })
        actions.addView(button("android_more") { more(actions) }); settingsButton = button("settings", edit = false) { showSettings() }; actions.addView(settingsButton); scrollRow(root, actions)
        val surveys = row(); survey = Spinner(this); surveys.addView(survey, LinearLayout.LayoutParams(0, dp(48), 1f)); editButtons.add(survey)
        surveys.addView(button("android_add") { nameDialog(true) }); compare = CheckBox(this).also { it.text = text["android_compare"]; it.setTextColor(palette.text); it.setOnCheckedChangeListener { _, checked -> if (!rendering) { if (checked) chooseBaseline() else model.compare(false, model.state.value.baselineId) } } }; surveys.addView(compare); editButtons.add(compare); root.addView(surveys)
        val network = row(); ap = Spinner(this); network.addView(ap, LinearLayout.LayoutParams(0, dp(48), 1f)); editButtons.add(ap); network.addView(button("scan") { model.scan(false) }); root.addView(network)
        val tools = row()
        for ((tool,key) in listOf(Tool.Point to "point_tool",Tool.Wall to "wall_tool",Tool.Scale to "scale_tool",Tool.Router to "router_tool")) tools.addView(button(key) { model.tool(tool) }.also { toolButtons[tool] = it })
        tools.addView(button("fit") { canvas.fit() }); tools.addView(button("undo") { model.undo() }); tools.addView(button("android_metrics") { chooseMetric() }); tools.addView(button("measured") { measurements() }); scrollRow(root, tools)
        selected = label(); selected.setTextColor(palette.muted); selected.setOnClickListener { details() }; root.addView(selected)
        canvas = PlanCanvas(this, model).also { it.contentDescription = text["android_canvas_hint"] }; root.addView(canvas, LinearLayout.LayoutParams(-1, 0, 1f))
        val gradient = View(this).also { it.background = GradientDrawable(GradientDrawable.Orientation.LEFT_RIGHT, intArrayOf(0xff66419c.toInt(),0xff4077b8.toInt(),0xff16a69a.toInt(),0xfff5c75d.toInt())).also { drawable -> drawable.cornerRadius=dp(4).toFloat() } }; root.addView(gradient,LinearLayout.LayoutParams(-1,dp(8)).also { it.setMargins(dp(8),dp(6),dp(8),0) })
        legend = label("",13f).also { it.setTextColor(palette.muted); it.setOnClickListener { mapExplanation() }; it.maxLines = 3 }; root.addView(legend)
        val command = row(); measure = button("measure", true) { model.scan(true) }; command.addView(measure,LinearLayout.LayoutParams(0,dp(52),1f)); cancel=button("cancel", edit=false){model.cancel()};command.addView(cancel);root.addView(command)
        status=label("",13f).also { it.setOnClickListener { val error=model.state.value.error;if(error!=null) dialog().setMessage(text[error.key]+if(error.detail.isBlank()) "" else "\n\n"+text["details"]+": "+error.detail).setPositiveButton(text["close"]){_,_->model.dismissError()}.show() } };root.addView(status)
        survey.onItemSelectedListener=listener { index -> model.state.value.project.surveys.getOrNull(index)?.let { model.selectSurvey(it.id) } }
        ap.onItemSelectedListener=listener { index -> model.state.value.aps.getOrNull(index)?.let { model.selectAp(it.bssid) } }
        setContentView(root)
        if(reopen) showSettings()
    }
    private fun listener(selected:(Int)->Unit)=object:AdapterView.OnItemSelectedListener {override fun onNothingSelected(parent:AdapterView<*>?){};override fun onItemSelected(parent:AdapterView<*>?,view:View?,position:Int,id:Long){if(!rendering)selected(position)}}
    private fun spinner(view:Spinner,values:List<String>,selected:Int){
        val previous=view.tag as? List<*>
        if(previous!=values){view.tag=values;view.adapter=object:ArrayAdapter<String>(this,android.R.layout.simple_spinner_item,values){
            override fun getView(position:Int,convertView:View?,parent:ViewGroup):View=super.getView(position,convertView,parent).also{(it as TextView).setTextColor(palette.text);it.isSingleLine=true}
            override fun getDropDownView(position:Int,convertView:View?,parent:ViewGroup):View=super.getDropDownView(position,convertView,parent).also{(it as TextView).setTextColor(palette.text);it.setBackgroundColor(palette.surface);it.setPadding(dp(12),dp(12),dp(12),dp(12))}
        }.also{it.setDropDownViewResource(android.R.layout.simple_spinner_dropdown_item)}}
        if(values.isNotEmpty()&&view.selectedItemPosition!=selected.coerceIn(values.indices))view.setSelection(selected.coerceIn(values.indices),false)
    }
    private fun TextView.setChanged(value:String){if(this.text.toString()!=value)this.text=value}
    private fun render(state:SurveyState){
        if(!::root.isInitialized||appearance?.language!=state.preferences.language||appearance?.dark!=state.preferences.dark)rebuild(state)
        rendering=true
        heading.setChanged("WifiMap · "+state.project.name.ifBlank{text["untitled"]}+if(state.dirty)" *" else "")
        spinner(survey,state.project.surveys.mapIndexed{i,s->s.name.ifBlank{text["survey"]+" "+(i+1)}},state.project.surveys.indexOfFirst{it.id==state.surveyId})
        val aps=state.aps;spinner(ap,if(aps.isEmpty())listOf(text["select_ap"])else aps.map{it.ssidLabel()+" · "+bssidLabel(it.bssid)},aps.indexOfFirst{it.bssid==state.ap}.coerceAtLeast(0))
        compare.isChecked=state.compare;compare.isEnabled=state.busy==null&&state.project.surveys.size>1
        editButtons.forEach{it.isEnabled=state.busy==null};compare.isEnabled=state.busy==null&&state.project.surveys.size>1
        toolButtons.forEach { (tool, button) -> val active = state.tool == tool; if (button.isSelected != active) { button.isSelected = active; button.background = border(if (active) palette.selected else palette.surface) } }
        measure.isEnabled=state.busy==null&&!state.project.demo&&state.point!=null&&state.ap!=null
        cancel.visibility=if(state.busy=="scanning"||state.update is UpdateStatus.Downloading)View.VISIBLE else View.GONE
        selected.setChanged(if(state.selected!=null)text["point_details"]+" · "+state.selected!!.observation.value(state.metric).let{if(it==null)text["unavailable"]else"$it ${state.metric.unit}"}else state.point?.let{"x ${it.x.toInt()} · y ${it.y.toInt()} px"}?:text["pick_point"])
        val difference=state.baseline!=null;val unit=if(difference)state.metric.differenceUnit else state.metric.unit
        legend.setChanged(text[if(difference)"difference"else"legend"]+" · "+(if(difference)"−20 … +20"else"${state.metric.minimum.toInt()} … ${state.metric.maximum.toInt()}")+" $unit\n"+text[if(difference)"measured_absolute"else"measured"]+" · "+text["unmeasured"]+if(state.calculating)" · "+text["working"]else if(state.heatmap?.cells?.any{it!=null}!=true)" · "+text["no_data"]else"")
        val message=state.error?.let{text[it.key]}?:state.busy?.let{text[it]}?:state.notice?.let{text[it]}?:if(state.project.demo)text["demo_badge"]else text["android_api_note"]
        status.setChanged(message);status.setTextColor(if(state.error!=null)if(state.preferences.dark)0xffffc280.toInt() else 0xff8f4423.toInt() else palette.muted)
        settingsButton.setChanged(text["settings"]+if(state.update is UpdateStatus.Available||state.update is UpdateStatus.Ready)" · "+text["update_available"]else"")
        canvas.update(state)
        rendering=false
        if(settingsDialog?.isShowing==true&&settingsKey!=Pair(state.preferences,state.update))renderSettings(state)
    }
    private fun result(uri:Uri?){if(!::model.isInitialized)return;val notices=runCatching{assets.open("LICENSE.txt").bufferedReader().use{it.readText()}+"\n\n"+assets.open("THIRD_PARTY_NOTICES.txt").bufferedReader().use{it.readText()}}.getOrDefault("");model.fileResult(uri,text,notices)}
    private fun effect(effect:UiEffect){when(effect){
        is UiEffect.PickFile->when(effect.action){FileAction.Open->projectPicker.launch(arrayOf("*/*"));FileAction.Import->imagePicker.launch(arrayOf("image/png","image/jpeg"));FileAction.Save->projectCreator.launch("WifiMap.wifimap");FileAction.Report->reportCreator.launch("WifiMap-report.svg");FileAction.Licenses->noticesCreator.launch("WifiMap-license-notices.txt")}
        is UiEffect.ConfirmLeave->dialog().setTitle(text["unsaved"]).setMessage(text["unsaved_detail"]).setPositiveButton(text["save"]){_,_->model.saveThen(effect.action)}.setNegativeButton(text["discard"]){_,_->model.navigate(effect.action,true)}.setNeutralButton(text["cancel"],null).show()
        is UiEffect.Distance->{val input=EditText(this).also{it.inputType=InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_FLAG_DECIMAL;it.setText("3.0")};dialog().setTitle(text["distance"]).setView(input).setPositiveButton(text["apply_scale"]){_,_->input.text.toString().replace(',','.').toFloatOrNull()?.let{model.setScale(effect.a,effect.b,it)}}.setNegativeButton(text["cancel"],null).show()}
        UiEffect.WifiPermission->if(model.hasPermissionRequest())dialog().setTitle(text["android_wifi_permission"]).setMessage(text["android_permission_reason"]).setPositiveButton(text["android_allow"]){_,_->permissions.launch(model.scanner.permissions())}.setNegativeButton(text["cancel"]){_,_->model.permissionResult(false)}.setOnCancelListener{model.permissionResult(false)}.show()
        UiEffect.InstallPermission->dialog().setMessage(text["android_install_permission"]).setPositiveButton(text["android_allow_install"]){_,_->startActivity(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,Uri.parse("package:$packageName")))}.setNegativeButton(text["cancel"],null).show()
        UiEffect.Close->finish()
    }}
    private fun dialog():AlertDialog.Builder {val theme=if(model.state.value.preferences.dark)android.R.style.Theme_Material_Dialog_Alert else android.R.style.Theme_Material_Light_Dialog_Alert;return AlertDialog.Builder(ContextThemeWrapper(this,theme))}
    private fun nameDialog(addSurvey:Boolean=false){val input=EditText(this).also{it.setText(if(addSurvey)text["survey"]+" "+(model.state.value.project.surveys.size+1)else model.state.value.project.name);it.filters=arrayOf(android.text.InputFilter.LengthFilter(100))};dialog().setTitle(text[if(addSurvey)"survey_name"else"plan_name"]).setView(input).setPositiveButton(text["save"]){_,_->if(addSurvey)model.addSurvey(input.text.toString())else model.setName(input.text.toString())}.setNegativeButton(text["cancel"],null).show()}
    private fun more(anchor:View){val menu=PopupMenu(this,anchor);listOf("new" to {model.navigate(Navigation.New)},"demo" to {model.navigate(Navigation.Example)},"export" to {model.pick(FileAction.Report)},"licenses_save" to {model.pick(FileAction.Licenses)}).forEach{(key,action)->menu.menu.add(text[key]).setOnMenuItemClickListener{action();true}};menu.show()}
    private fun chooseBaseline(){val s=model.state.value;val options=s.project.surveys.filter{it.id!=s.surveyId};dialog().setTitle(text["baseline"]).setItems(options.map{it.name.ifBlank{text["survey"]+" "+it.id}}.toTypedArray()){_,i->model.compare(true,options[i].id)}.setNegativeButton(text["cancel"]){_,_->render(model.state.value)}.show()}
    private fun chooseMetric(){val items=arrayOf(text["rssi"],text["quality"],text["radius"]);dialog().setTitle(text["metric"]).setItems(items){_,which->if(which<2)model.metric(Metric.entries[which])else{val input=EditText(this).also{it.inputType=InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_FLAG_DECIMAL;it.setText(model.state.value.radiusMeters.toString())};dialog().setTitle(text["radius"]).setView(input).setPositiveButton(text["save"]){_,_->input.text.toString().toFloatOrNull()?.let(model::radius)}.setNegativeButton(text["cancel"],null).show()}}.show()}
    private fun measurements(){val s=model.state.value;val rows=s.survey.samples.filter{it.observation.bssid==s.ap};dialog().setTitle(text["measured"]).setItems(rows.mapIndexed{i,sample->"${i+1} · ${sample.observation.value(s.metric)?:"—"} ${s.metric.unit}"}.toTypedArray()){_,i->model.select(rows[i].id);details()}.setNegativeButton(text["close"],null).show()}
    private fun details(){val sample=model.state.value.selected?:return;val o=sample.observation;val source=text[when(o.source){ObservationSource.WindowsBss->"android_source_windows";ObservationSource.AndroidScan->"android_source_android";ObservationSource.Demo->"demo_badge"}];val value=buildString{appendLine(o.ssidLabel());appendLine(bssidLabel(o.bssid));appendLine(source);appendLine("${text["rssi"]}: ${o.rssiDbm?:text["unavailable"]}");appendLine("${text["quality"]}: ${o.linkQualityPercent?:text["unavailable"]}");appendLine("${text["frequency"]}: ${o.frequencyKhz?.div(1000)?:"—"} MHz · ${text["channel"]}: ${o.channel?:"—"}");appendLine(text["timestamp"]+": "+Instant.ofEpochMilli(o.observedUnixMs));appendLine(text["collected"]+": "+Instant.ofEpochMilli(o.collectedUnixMs));if(o.ssidEncoding==SsidEncoding.Utf8Display)appendLine(text["android_ssid_display"])};dialog().setTitle(text["point_details"]).setMessage(value).setNegativeButton(text["remove_point"]){_,_->model.removeSelected()}.setPositiveButton(text["close"],null).show()}
    private fun mapExplanation(){val s=model.state.value;val keys=listOf("estimate_note","confidence_note",if(s.baseline!=null)"compare_note"else"android_api_note",if(s.metric==Metric.LinkQualityPercent&&s.baseline!=null)"quality_delta_note"else"android_touch_hint");dialog().setTitle(text["legend"]).setMessage(keys.joinToString("\n\n"){text[it]}).setPositiveButton(text["close"],null).show()}
    private fun showSettings(){val body=column();body.setPadding(dp(12),dp(8),dp(12),dp(8));val scroll=ScrollView(this).also{it.addView(body)};settingsBody=body;settingsKey=null;settingsDialog=dialog().setTitle(text["settings"]).setView(scroll).setPositiveButton(text["close"],null).create().also{it.show()};renderSettings(model.state.value)}
    private fun renderSettings(s:SurveyState){val body=settingsBody?:return;settingsKey=Pair(s.preferences,s.update);body.removeAllViews()
        fun check(key:String,value:Boolean,action:(Boolean)->Unit){body.addView(CheckBox(this).also{it.text=text[key];it.setTextColor(palette.text);it.isChecked=value;it.setOnCheckedChangeListener{_,checked->action(checked)}})}
        body.addView(label(text["appearance"],19f));check("dark",s.preferences.dark){model.preferences(s.preferences.copy(dark=it))};check("reduced_motion",s.preferences.reducedMotion){model.preferences(s.preferences.copy(reducedMotion=it))}
        val languages=listOf("English","Русский","Français","Deutsch","Español","Italiano");val codes=listOf("en","ru","fr","de","es","it");body.addView(button("language",edit=false){dialog().setTitle(text["language"]).setItems(languages.toTypedArray()){_,index->model.preferences(model.state.value.preferences.copy(language=codes[index]))}.show()})
        body.addView(label(text["updates"],19f));body.addView(label(text["installed_version"]+": "+BuildConfig.VERSION_NAME+if(BuildConfig.DEBUG)" · "+text["android_debug_build"]else""));check("startup_check",s.preferences.startupUpdates){model.preferences(s.preferences.copy(startupUpdates=it))}
        body.addView(button("check_updates",edit=false){model.checkUpdates(true)}.also{it.isEnabled=s.update !is UpdateStatus.Checking&&s.update !is UpdateStatus.Downloading})
        when(val update=s.update){UpdateStatus.Idle->{};UpdateStatus.Checking->body.addView(label(text["working"]));UpdateStatus.Current->body.addView(label(text["update_current"]));is UpdateStatus.Available->{body.addView(label(text["update_available"]+" · "+update.release.version));body.addView(button("download_install",edit=false){model.downloadUpdate()})};is UpdateStatus.Downloading->{body.addView(label(text["downloading"]+" ${update.percent}%"));body.addView(ProgressBar(this,null,android.R.attr.progressBarStyleHorizontal).also{it.max=100;it.progress=update.percent});body.addView(button("cancel",edit=false){model.cancel()})};is UpdateStatus.Ready->{body.addView(label(text["android_update_ready"]));body.addView(button("download_install",edit=false){model.installUpdate()})};is UpdateStatus.Failed->body.addView(label(text[update.error.key]+if(update.error.detail.isBlank())""else"\n"+update.error.detail))}
        body.addView(label(text["update_privacy"]));body.addView(label(text["licenses_detail"]));body.addView(button("licenses_save",edit=false){model.pick(FileAction.Licenses)})
    }
}
