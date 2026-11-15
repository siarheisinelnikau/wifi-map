package com.siarheisinelnikau.wifimap.presentation

import android.graphics.Bitmap
import android.net.Uri
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.siarheisinelnikau.wifimap.data.*
import com.siarheisinelnikau.wifimap.domain.*
import com.siarheisinelnikau.wifimap.platform.AndroidWifiScanner
import kotlinx.coroutines.*
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.*

enum class Tool { Point, Wall, Scale, Router }
enum class Navigation { New, Example, Open, Import, Close }
enum class FileAction { Open, Import, Save, Report, Licenses }
sealed interface UiEffect {
    data class PickFile(val action: FileAction) : UiEffect
    data class ConfirmLeave(val action: Navigation) : UiEffect
    data class Distance(val a: Point, val b: Point) : UiEffect
    data object WifiPermission : UiEffect
    data object InstallPermission : UiEffect
    data object Close : UiEffect
}
sealed interface UpdateStatus {
    data object Idle : UpdateStatus
    data object Checking : UpdateStatus
    data object Current : UpdateStatus
    data class Available(val release: AndroidRelease) : UpdateStatus
    data class Downloading(val percent: Int) : UpdateStatus
    data class Ready(val apk: VerifiedApk) : UpdateStatus
    data class Failed(val error: SurveyFailure) : UpdateStatus
}
data class SurveyState(
    val project: WifiProject = WifiProject(), val revision: Long = 0, val dirty: Boolean = false,
    val surveyId: Long = 1, val baselineId: Long? = null, val compare: Boolean = false,
    val ap: List<Int>? = null, val accessPoints: List<Observation> = emptyList(), val metric: Metric = Metric.RssiDbm,
    val radiusMeters: Float = 3f, val point: Point? = null, val selectedId: Long? = null,
    val tool: Tool = Tool.Point, val anchor: Point? = null, val heatmap: Heatmap? = null,
    val calculating: Boolean = false, val busy: String? = null, val error: SurveyFailure? = null,
    val notice: String? = null, val planBitmap: Bitmap? = null, val preferences: UserPreferences = UserPreferences(),
    val update: UpdateStatus = UpdateStatus.Idle, val canUndo: Boolean = false,
) {
    val survey: Survey get() = project.surveys.first { it.id == surveyId }
    val baseline: Long? get() = baselineId?.takeIf { compare && it != surveyId }
    val selected: Sample? get() = survey.samples.find { it.id == selectedId }
    val aps: List<Observation> get() = (survey.samples.map { it.observation } + accessPoints).distinctBy { it.bssid }
}

class SurveyViewModel(val store: ProjectStore, val scanner: AndroidWifiScanner, private val preferences: Preferences, val updater: AndroidUpdater) : ViewModel() {
    private val mutable = MutableStateFlow(SurveyState(preferences = preferences.load()))
    val state: StateFlow<SurveyState> = mutable.asStateFlow()
    private val events = Channel<UiEffect>(Channel.BUFFERED)
    val effects = events.receiveAsFlow()
    private var operation: Job? = null; private var computation: Job? = null; private var checkpoint: Job? = null; private var updateJob: Job? = null
    private var generation = 0L; private var pendingNavigation: Navigation? = null; private var pendingFile: FileAction? = null
    private var pendingScan: MeasurementTarget? = null; private var pendingRefresh = false; private var previous: WifiProject? = null
    private var navigateAfterJob: Navigation? = null
    init {
        launchOperation("working") { store.recovery()?.let { project -> replace(project, true); notice("android_recovered") } }
        checkUpdates(false)
    }
    private fun emit(effect: UiEffect) { viewModelScope.launch { events.send(effect) } }
    private fun notice(key: String) { mutable.update { it.copy(notice = key, error = null) } }
    fun error(failure: Throwable) { if (failure !is CancellationException) mutable.update { it.copy(error = failure as? SurveyFailure ?: SurveyFailure("file_error", failure.javaClass.simpleName), notice = null) } }
    fun dismissError() { mutable.update { it.copy(error = null, notice = null) } }
    fun installationResult(key: String, detail: String) { if (key == "android_update_installed") notice(key) else error(SurveyFailure(key, detail)) }
    private fun launchOperation(label: String, task: suspend () -> Unit) {
        if (state.value.busy != null) return
        mutable.update { it.copy(busy = label, error = null, notice = null) }
        operation = viewModelScope.launch {
            try { task() } catch (_: CancellationException) { pendingNavigation = null; notice("cancelled") } catch (failure: Throwable) { pendingNavigation = null; error(failure) }
            finally { mutable.update { it.copy(busy = null) }; val next = navigateAfterJob; navigateAfterJob = null; if (next != null) navigate(next, true) }
        }
    }
    private suspend fun replace(project: WifiProject, dirty: Boolean) {
        val bitmap = store.displayImage(project.plan)
        computation?.cancel(); checkpoint?.cancel(); generation++
        val before = state.value
        previous = null
        mutable.value = SurveyState(project, before.revision + 1, dirty, project.surveys.first().id, project.surveys.getOrNull(1)?.id, ap = project.surveys.first().samples.firstOrNull()?.observation?.bssid, planBitmap = bitmap, preferences = before.preferences, update = before.update, busy = before.busy)
        scheduleCheckpoint(); recompute()
    }
    fun navigate(action: Navigation, confirmed: Boolean = false) {
        if (state.value.busy != null) { if (action == Navigation.Close) { cancel(); if (state.value.dirty && !confirmed) emit(UiEffect.ConfirmLeave(action)) else emit(UiEffect.Close) }; return }
        if (state.value.dirty && !confirmed) { emit(UiEffect.ConfirmLeave(action)); return }
        pendingNavigation = null
        when (action) {
            Navigation.Close -> if (confirmed && state.value.dirty) {
                checkpoint?.cancel()
                launchOperation("working") { store.clearRecovery(); emit(UiEffect.Close) }
            } else emit(UiEffect.Close)
            Navigation.Open -> pick(FileAction.Open)
            Navigation.Import -> pick(FileAction.Import)
            Navigation.New -> launchOperation("working") { replace(WifiProject(), false) }
            Navigation.Example -> launchOperation("working") { replace(exampleProject(), false) }
        }
    }
    fun saveThen(action: Navigation) { pendingNavigation = action; pick(FileAction.Save) }
    fun pick(action: FileAction) {
        if (state.value.busy != null) return
        pendingFile = action; mutable.update { it.copy(busy = "android_pick_file", error = null) }; emit(UiEffect.PickFile(action))
    }
    fun fileResult(uri: Uri?, text: Text, licenses: String) {
        val action = pendingFile ?: return
        pendingFile = null; mutable.update { it.copy(busy = null) }
        if (uri == null) { pendingNavigation = null; return }
        val snapshot = state.value
        launchOperation("working") {
            when (action) {
                FileAction.Open -> replace(store.open(uri), false)
                FileAction.Import -> replace(WifiProject(plan = store.importImage(uri)), true)
                FileAction.Save -> {
                    store.save(uri, snapshot.project)
                    if (state.value.revision == snapshot.revision) mutable.update { it.copy(dirty = false, notice = "saved") }
                    navigateAfterJob = pendingNavigation; pendingNavigation = null
                }
                FileAction.Report -> {
                    val ap = snapshot.ap ?: throw SurveyFailure("select_ap")
                    val report = withContext(Dispatchers.Default) { SvgReport.create(snapshot.project, snapshot.surveyId, snapshot.baseline, ap, snapshot.metric, snapshot.heatmap) { text[it] } }
                    store.writeText(uri, report); notice("exported")
                }
                FileAction.Licenses -> { store.writeText(uri, licenses); notice("licenses_saved") }
            }
        }
    }
    private fun change(project: WifiProject, heat: Boolean = true) {
        if (state.value.busy != null) return
        previous = state.value.project; pendingNavigation = null
        mutable.update { it.copy(project = project, revision = it.revision + 1, dirty = true, canUndo = true, notice = null) }
        scheduleCheckpoint(); if (heat) recompute()
    }
    private fun scheduleCheckpoint() {
        checkpoint?.cancel(); val snapshot = state.value.project
        checkpoint = viewModelScope.launch { delay(400); try { store.checkpoint(snapshot) } catch (failure: Throwable) { error(failure) } }
    }
    fun undo() { if (state.value.busy != null) return; val project = previous ?: return; previous = null; mutable.update { it.copy(project = project, surveyId = project.surveys.firstOrNull { s -> s.id == it.surveyId }?.id ?: project.surveys.first().id, revision = it.revision + 1, dirty = true, canUndo = false, selectedId = null) }; scheduleCheckpoint(); recompute() }
    fun setName(name: String) = change(state.value.project.copy(name = name.take(100)), false)
    fun addSurvey(name: String) {
        try { val old = state.value; val project = SurveyRules.addSurvey(old.project, name.take(100)); change(project, false); mutable.update { it.copy(surveyId = project.surveys.last().id, baselineId = old.surveyId, selectedId = null, point = null) }; recompute() } catch (failure: Throwable) { error(failure) }
    }
    fun selectSurvey(id: Long) {
        if (state.value.busy != null || state.value.surveyId == id) return
        val old = state.value.surveyId
        mutable.update { s -> s.copy(surveyId = id, baselineId = if (s.baselineId == id) old else s.baselineId, point = null, selectedId = null, ap = s.project.surveys.first { it.id == id }.samples.firstOrNull()?.observation?.bssid ?: s.ap) }; recompute()
    }
    fun compare(enabled: Boolean, baseline: Long?) { if (state.value.busy != null) return; mutable.update { it.copy(compare = enabled, baselineId = baseline) }; recompute() }
    fun selectAp(ap: List<Int>) { if (state.value.busy != null || state.value.ap == ap) return; mutable.update { it.copy(ap = ap, selectedId = null) }; recompute() }
    fun metric(metric: Metric) { if (state.value.busy != null) return; mutable.update { it.copy(metric = metric) }; recompute() }
    fun radius(value: Float) { if (state.value.busy != null) return; mutable.update { it.copy(radiusMeters = value.coerceIn(1f, 10f)) }; recompute() }
    fun tool(tool: Tool) { if (state.value.busy != null) return; mutable.update { it.copy(tool = tool, anchor = null) } }
    fun select(id: Long) { val sample = state.value.survey.samples.find { it.id == id } ?: return; mutable.update { it.copy(selectedId = id, point = sample.position) } }
    fun tap(point: Point, hit: Long?) {
        val s = state.value; if (s.busy != null || !point.inside(s.project.plan.size)) return
        when (s.tool) {
            Tool.Point -> if (hit != null) select(hit) else mutable.update { it.copy(point = point, selectedId = null) }
            Tool.Router -> change(s.project.copy(surveys = s.project.surveys.map { if (it.id == s.surveyId) it.copy(router = point) else it }), false)
            Tool.Wall -> if (s.anchor == null) mutable.update { it.copy(anchor = point) } else if (s.anchor.distance(point) >= 1 && s.project.plan.walls.size < 10_000) { change(s.project.copy(plan = s.project.plan.copy(walls = s.project.plan.walls + Wall(s.anchor, point))), false); mutable.update { it.copy(anchor = null) } }
            Tool.Scale -> if (s.anchor == null) mutable.update { it.copy(anchor = point) } else if (s.anchor.distance(point) >= 1) { mutable.update { it.copy(anchor = null) }; emit(UiEffect.Distance(s.anchor, point)) }
        }
    }
    fun setScale(a: Point, b: Point, meters: Float) { val scale = Calibration(a, b, meters); if (scale.metersPerPixel() == null) { error(SurveyFailure("image_error")); return }; change(state.value.project.copy(plan = state.value.project.plan.copy(calibration = scale))); tool(Tool.Point) }
    fun move(id: Long, point: Point) { val s = state.value; if (!point.inside(s.project.plan.size)) return; change(SurveyRules.move(s.project, s.surveyId, id, point)); mutable.update { it.copy(selectedId = id, point = point) } }
    fun removeSelected() { val s = state.value; val id = s.selectedId ?: return; change(s.project.copy(surveys = s.project.surveys.map { if (it.id == s.surveyId) it.copy(samples = it.samples.filterNot { it.id == id }) else it })); mutable.update { it.copy(selectedId = null) } }
    fun scan(measure: Boolean) {
        val s = state.value; if (s.busy != null) return
        if (s.project.demo) { error(SurveyFailure("demo_isolated")); return }
        if (measure && (s.point == null || s.ap == null)) return
        pendingScan = if (measure) MeasurementTarget(s.surveyId, s.point!!, s.ap!!) else null; pendingRefresh = !measure
        if (!scanner.permissionGranted()) { mutable.update { it.copy(busy = "android_permission") }; emit(UiEffect.WifiPermission) } else performScan()
    }
    fun permissionResult(granted: Boolean) { mutable.update { it.copy(busy = null) }; if (granted && scanner.permissionGranted()) performScan() else { pendingScan = null; pendingRefresh = false; error(SurveyFailure("android_permission")) } }
    private fun performScan() {
        val target = pendingScan; val refresh = pendingRefresh; pendingScan = null; pendingRefresh = false
        if (target == null && !refresh) return
        launchOperation("scanning") {
            if (target == null) { val observations = scanner.scan(null); mutable.update { it.copy(accessPoints = observations, ap = it.ap?.takeIf { ap -> observations.any { o -> o.bssid == ap } } ?: observations.first().bssid) }; recompute() }
            else {
                val observation = CollectMeasurement(scanner)(target)
                val current = state.value
                val project = withContext(Dispatchers.Default) { SurveyRules.record(current.project, target.surveyId, target.position, observation) }
                previous = current.project
                mutable.update { it.copy(project = project, dirty = true, revision = it.revision + 1, point = target.position, selectedId = project.nextId - 1, ap = observation.bssid, canUndo = true) }
                scheduleCheckpoint(); recompute()
            }
        }
    }
    fun recompute() {
        computation?.cancel(); val tag = ++generation; val s = state.value
        mutable.update { it.copy(heatmap = null, calculating = s.ap != null) }
        val ap = s.ap ?: return
        computation = viewModelScope.launch {
            val worker = coroutineContext[Job]!!
            val map = withContext(Dispatchers.Default) { Heatmaps.compute(s.project, s.surveyId, s.baseline, ap, s.metric, s.radiusMeters) { !worker.isActive } }
            if (tag == generation && isActive) mutable.update { it.copy(heatmap = map, calculating = false) }
        }
    }
    fun preferences(value: UserPreferences) { preferences.save(value); mutable.update { it.copy(preferences = value) } }
    fun checkUpdates(manual: Boolean) {
        UpdateRules.dispatch(manual, state.value.preferences.startupUpdates) {
            if (updateJob?.isActive == true) return@dispatch
            mutable.update { it.copy(update = UpdateStatus.Checking) }
            updateJob = viewModelScope.launch {
                try { val available = updater.check(); mutable.update { it.copy(update = if (available != null) UpdateStatus.Available(available) else if (manual) UpdateStatus.Current else UpdateStatus.Idle) } }
                catch (failure: Throwable) { if (failure !is CancellationException) mutable.update { it.copy(update = if (manual) UpdateStatus.Failed(failure as? SurveyFailure ?: SurveyFailure("update_error", failure.javaClass.simpleName)) else UpdateStatus.Idle) } }
            }
        }
    }
    fun downloadUpdate() {
        val available = (state.value.update as? UpdateStatus.Available)?.release ?: return
        mutable.update { it.copy(update = UpdateStatus.Downloading(0)) }
        updateJob = viewModelScope.launch {
            try { val apk = updater.download(available) { percent -> mutable.update { it.copy(update = UpdateStatus.Downloading(percent)) } }; mutable.update { it.copy(update = UpdateStatus.Ready(apk)) } }
            catch (failure: Throwable) { mutable.update { it.copy(update = UpdateStatus.Failed(failure as? SurveyFailure ?: SurveyFailure(if (failure is CancellationException) "cancelled" else "update_error", failure.javaClass.simpleName))) } }
        }
    }
    fun installUpdate() {
        val apk = (state.value.update as? UpdateStatus.Ready)?.apk ?: return
        if (!updater.canInstall()) { emit(UiEffect.InstallPermission); return }
        launchOperation("working") { store.checkpoint(state.value.project); updater.install(apk) }
    }
    fun hasPermissionRequest() = state.value.busy == "android_permission" && (pendingScan != null || pendingRefresh)
    fun cancel() { pendingScan = null; pendingRefresh = false; if (state.value.busy == "android_permission") mutable.update { it.copy(busy = null) }; if (state.value.busy == "scanning") operation?.cancel(); if (state.value.update is UpdateStatus.Downloading) updateJob?.cancel() }
    fun onBackground() { cancel(); computation?.cancel(); generation++; mutable.update { it.copy(calculating = false) }; scheduleCheckpoint() }
    fun onForeground() { if (state.value.ap != null && state.value.heatmap == null) recompute() }
    override fun onCleared() { operation?.cancel(); computation?.cancel(); checkpoint?.cancel(); updateJob?.cancel(); events.close(); super.onCleared() }
}
