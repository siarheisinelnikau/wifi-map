package com.siarheisinelnikau.wifimap.presentation

import android.animation.ValueAnimator
import android.content.Context
import android.graphics.*
import android.view.*
import com.siarheisinelnikau.wifimap.domain.*
import com.siarheisinelnikau.wifimap.domain.Point
import kotlin.math.abs

class PlanCanvas(context: Context, private val model: SurveyViewModel) : View(context) {
    private var scene = model.state.value
    private var heatBitmap: Bitmap? = null
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG or Paint.FILTER_BITMAP_FLAG)
    private val transform = ViewTransform()
    private val density = resources.displayMetrics.density
    private val font = android.util.TypedValue.applyDimension(android.util.TypedValue.COMPLEX_UNIT_SP, 1f, resources.displayMetrics)
    private var fitNeeded = true
    private var down = Point(0f, 0f); private var last = down; private var moving = false; private var multitouch = false
    private var dragId: Long? = null; private var preview: Point? = null
    private var pulse = 1f; private var animator: ValueAnimator? = null
    private val scale = ScaleGestureDetector(context, object : ScaleGestureDetector.SimpleOnScaleGestureListener() {
        override fun onScale(detector: ScaleGestureDetector): Boolean { transform.zoomAt(Point(detector.focusX, detector.focusY), detector.scaleFactor); redraw(); return true }
    })
    init { isFocusable = true; isClickable = true; setLayerType(LAYER_TYPE_SOFTWARE, null); contentDescription = "WifiMap" }
    private fun redraw() { if (isShown && windowVisibility == VISIBLE) invalidate() }
    fun update(value: SurveyState) {
        val before = scene; scene = value
        if (before.project.plan !== value.project.plan && (before.project.plan.size != value.project.plan.size || before.project.plan.image != value.project.plan.image)) fitNeeded = true
        if (before.heatmap !== value.heatmap || (heatBitmap == null && value.heatmap != null)) { heatBitmap?.recycle(); heatBitmap = value.heatmap?.let { Bitmap.createBitmap(it.argb(), it.width, it.height, Bitmap.Config.ARGB_8888) } }
        if (before.selectedId != value.selectedId || before.point != value.point) {
            animator?.cancel()
            if (value.preferences.reducedMotion || !ValueAnimator.areAnimatorsEnabled() || !isShown) pulse = 1f else {
                animator = ValueAnimator.ofFloat(0f, 1f).also { it.duration = 180; it.addUpdateListener { animation -> pulse = animation.animatedValue as Float; redraw() }; it.start() }
            }
        }
        if (before.project !== value.project || before.heatmap !== value.heatmap || before.point != value.point || before.anchor != value.anchor || before.selectedId != value.selectedId || before.surveyId != value.surveyId || before.ap != value.ap || before.preferences != value.preferences) redraw()
    }
    fun fit() { fitNeeded = true; redraw() }
    fun stopMotion() { animator?.cancel(); animator = null; pulse = 1f }
    override fun onDetachedFromWindow() { stopMotion(); heatBitmap?.recycle(); heatBitmap = null; super.onDetachedFromWindow() }
    override fun onWindowVisibilityChanged(visibility: Int) { super.onWindowVisibilityChanged(visibility); if (visibility != VISIBLE) stopMotion() }
    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) { fitNeeded = true }
    private fun screen(point: Point) = transform.toScreen(point)
    private fun hit(point: Point): Long? = scene.survey.samples.filter { it.observation.bssid == scene.ap }.minByOrNull { it.position.distance(point) }?.takeIf { it.position.distance(point) * transform.zoom < 18 * density }?.id
    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas); val s = scene; val palette = Palette.of(s.preferences.dark); canvas.drawColor(palette.background)
        if (width <= 0 || height <= 0) return
        if (fitNeeded) {
            transform.zoom = minOf((width - 32 * density) / s.project.plan.size[0], (height - 32 * density) / s.project.plan.size[1]).coerceAtLeast(0.02f)
            transform.origin = Point((width - s.project.plan.size[0] * transform.zoom) / 2, (height - s.project.plan.size[1] * transform.zoom) / 2); fitNeeded = false
        }
        val origin = screen(Point(0f,0f)); val end = screen(Point(s.project.plan.size[0].toFloat(), s.project.plan.size[1].toFloat())); val area = RectF(origin.x, origin.y, end.x, end.y)
        paint.style = Paint.Style.FILL; paint.color = palette.surface; canvas.drawRoundRect(area, 5*density, 5*density, paint)
        s.planBitmap?.let { paint.color = Color.WHITE; canvas.drawBitmap(it, null, area, paint) }
        if (s.planBitmap == null) {
            paint.color = palette.border
            val step = 50 * transform.zoom
            if (step >= 14 * density) {
                var x = origin.x; while (x < end.x) { if (x in -2f..width.toFloat()+2) { var y = origin.y; while (y < end.y) { if (y in -2f..height.toFloat()+2) canvas.drawCircle(x,y,density,paint); y+=step } }; x+=step }
            }
        }
        heatBitmap?.let { paint.color = Color.WHITE; canvas.drawBitmap(it, null, area, paint) }
        s.project.plan.walls.forEach { wall -> val a=screen(wall.a);val b=screen(wall.b);paint.strokeCap=Paint.Cap.ROUND;paint.strokeWidth=(6*transform.zoom).coerceIn(2*density,8*density);paint.color=palette.text
            if (s.planBitmap != null) { paint.strokeWidth += 2*density;paint.color=palette.background;canvas.drawLine(a.x,a.y,b.x,b.y,paint);paint.strokeWidth-=2*density;paint.color=palette.text };canvas.drawLine(a.x,a.y,b.x,b.y,paint) }
        s.survey.router?.let { val p=screen(it);paint.color=palette.accent;canvas.drawRoundRect(RectF(p.x-10*density,p.y-10*density,p.x+10*density,p.y+10*density),4*density,4*density,paint);paint.color=Color.WHITE;paint.textSize=13*font;paint.textAlign=Paint.Align.CENTER;canvas.drawText("R",p.x,p.y+4*font,paint);paint.textAlign=Paint.Align.LEFT }
        s.survey.samples.forEachIndexed { index,sample -> if (sample.observation.bssid == s.ap) {
            val at = screen(if(sample.id==dragId) preview?:sample.position else sample.position)
            if(at.x < -30*density || at.x>width+30*density || at.y< -30*density || at.y>height+30*density) return@forEachIndexed
            paint.style=Paint.Style.FILL;paint.color=sample.observation.value(s.metric)?.let { 0xff000000.toInt() or Heatmaps.color(it,s.metric,false) }?:palette.muted;canvas.drawCircle(at.x,at.y,6.5f*density,paint)
            paint.style=Paint.Style.STROKE;paint.color=Color.WHITE;paint.strokeWidth=2*density;canvas.drawCircle(at.x,at.y,6.5f*density,paint)
            paint.style=Paint.Style.FILL;paint.textSize=12*font;val label=(index+1).toString();val left=at.x+10*density;val top=at.y-20*density
            paint.color=palette.surface;canvas.drawRoundRect(RectF(left-2*density,top,left+paint.measureText(label)+4*density,top+16*font),3*density,3*density,paint);paint.color=palette.text;canvas.drawText(label,left,top+12*font,paint)
        } }
        s.point?.let { val at=screen(preview?:it);paint.style=Paint.Style.STROKE;paint.strokeWidth=1.8f*density;paint.color=palette.accent;canvas.drawCircle(at.x,at.y,(13+5*(1-pulse)*(1-pulse))*density,paint);if(s.selectedId==null){canvas.drawLine(at.x-6*density,at.y,at.x+6*density,at.y,paint);canvas.drawLine(at.x,at.y-6*density,at.x,at.y+6*density,paint)} }
        s.anchor?.let { val a=screen(it);paint.style=Paint.Style.FILL;paint.color=palette.accent;canvas.drawCircle(a.x,a.y,5*density,paint) }
        s.project.plan.calibration?.metersPerPixel()?.let { val length=transform.zoom/it;if(length in 20*density..width*0.7f){paint.style=Paint.Style.STROKE;paint.color=palette.accent;paint.strokeWidth=3*density;canvas.drawLine(16*density,height-20*density,16*density+length,height-20*density,paint);paint.style=Paint.Style.FILL;paint.textSize=12*font;canvas.drawText("1 m",16*density,height-28*density,paint)} }
        paint.style=Paint.Style.FILL
    }
    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (scene.busy != null) return false
        scale.onTouchEvent(event)
        val point=Point(event.x,event.y)
        when(event.actionMasked) {
            MotionEvent.ACTION_DOWN -> { requestFocus();parent.requestDisallowInterceptTouchEvent(true);down=point;last=point;moving=false;multitouch=false;dragId=if(scene.tool==Tool.Point) hit(transform.toPlan(point)) else null }
            MotionEvent.ACTION_POINTER_DOWN -> { multitouch=true;dragId=null;preview=null;last=point }
            MotionEvent.ACTION_MOVE -> {
                if(abs(point.x-down.x)+abs(point.y-down.y)>ViewConfiguration.get(context).scaledTouchSlop) moving=true
                if(multitouch || (moving && dragId==null)) { if(!scale.isInProgress){transform.origin=Point(transform.origin.x+point.x-last.x,transform.origin.y+point.y-last.y);redraw()} }
                else if(moving && dragId!=null) { val p=transform.toPlan(point);preview=Point(p.x.coerceIn(0f,scene.project.plan.size[0].toFloat()),p.y.coerceIn(0f,scene.project.plan.size[1].toFloat()));redraw() }
                last=point
            }
            MotionEvent.ACTION_UP -> {
                if(!multitouch && moving && dragId!=null && preview!=null) model.move(dragId!!,preview!!)
                else if(!moving && !multitouch){val p=transform.toPlan(point);model.tap(p,hit(p));performClick()}
                dragId=null;preview=null;parent.requestDisallowInterceptTouchEvent(false);redraw()
            }
            MotionEvent.ACTION_CANCEL -> {dragId=null;preview=null;parent.requestDisallowInterceptTouchEvent(false);redraw()}
        };return true
    }
    override fun performClick():Boolean { super.performClick();return true }
    override fun onKeyDown(keyCode:Int,event:KeyEvent):Boolean {
        if(scene.busy!=null)return super.onKeyDown(keyCode,event)
        val current=scene.point?:Point(scene.project.plan.size[0]/2f,scene.project.plan.size[1]/2f)
        val next=when(keyCode){KeyEvent.KEYCODE_DPAD_LEFT->current.copy(x=current.x-10);KeyEvent.KEYCODE_DPAD_RIGHT->current.copy(x=current.x+10);KeyEvent.KEYCODE_DPAD_UP->current.copy(y=current.y-10);KeyEvent.KEYCODE_DPAD_DOWN->current.copy(y=current.y+10);KeyEvent.KEYCODE_ENTER,KeyEvent.KEYCODE_DPAD_CENTER->{model.scan(true);return true};else->null}
        if(next!=null){model.tap(Point(next.x.coerceIn(0f,scene.project.plan.size[0].toFloat()),next.y.coerceIn(0f,scene.project.plan.size[1].toFloat())),null);return true};return super.onKeyDown(keyCode,event)
    }
}
