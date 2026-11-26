package org.ytmrs.android;

import android.app.Activity;
import android.app.Dialog;
import android.content.Context;
import android.content.SharedPreferences;
import android.graphics.drawable.ColorDrawable;
import android.graphics.drawable.GradientDrawable;
import android.os.Handler;
import android.os.Looper;
import android.text.Editable;
import android.text.TextUtils;
import android.text.TextWatcher;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.KeyEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.Window;
import android.view.WindowManager;
import android.view.inputmethod.EditorInfo;
import android.widget.ArrayAdapter;
import android.widget.Button;
import android.widget.EditText;
import android.widget.ImageButton;
import android.widget.LinearLayout;
import android.widget.ListView;
import android.widget.TextView;

import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.concurrent.atomic.AtomicLong;
import java.util.function.Consumer;

import org.json.JSONArray;
import org.json.JSONException;

/** Full-window music search with a native input field and suggestions above the keyboard. */
final class MusicSearchDialog extends Dialog {
    interface SuggestionRequest {
        void send(long revision, String query);
    }

    private static final AtomicLong REVISIONS = new AtomicLong();
    private final Handler main = new Handler(Looper.getMainLooper());
    private final SharedPreferences preferences;
    private final List<String> recent = new ArrayList<>();
    private final SuggestionRequest request;
    private final ArrayAdapter<String> items;
    private final LinearLayout recentHeader;
    private final EditText field;
    private Runnable debounce;
    private long revision;
    private String query = "";
    private String selected;

    MusicSearchDialog(Activity activity, boolean light, SuggestionRequest request, Consumer<String> finish) {
        super(activity, light ? android.R.style.Theme_Material_Light_NoActionBar : android.R.style.Theme_Material_NoActionBar);
        Context context = getContext();
        this.request = request;
        preferences = activity.getPreferences(Context.MODE_PRIVATE);
        try {
            JSONArray saved = new JSONArray(preferences.getString("recentSearches", "[]"));
            for (int i = 0; i < saved.length() && i < 15; i++) recent.add(saved.getString(i));
        } catch (JSONException ignored) {}

        int background = light ? 0xfffafafa : 0xff101010;
        int contrast = light ? 0xff181818 : 0xfff5f5f5;
        int muted = light ? 0xff555555 : 0xffaaaaaa;
        int fill = light ? 0xffebebeb : 0xff282828;
        LinearLayout column = new LinearLayout(context);
        column.setOrientation(LinearLayout.VERTICAL);
        column.setBackgroundColor(background);

        LinearLayout toolbar = new LinearLayout(context);
        toolbar.setGravity(Gravity.CENTER_VERTICAL);
        toolbar.setPaddingRelative(dp(4), 0, dp(4), 0);
        column.addView(toolbar, new LinearLayout.LayoutParams(-1, dp(64)));
        ImageButton back = icon(R.drawable.search_back, "Back", contrast);
        toolbar.addView(back, new LinearLayout.LayoutParams(dp(48), dp(48)));
        back.setOnClickListener(view -> dismiss());

        GradientDrawable rounded = new GradientDrawable();
        rounded.setColor(fill);
        rounded.setCornerRadius(dp(22));
        LinearLayout input = new LinearLayout(context);
        input.setGravity(Gravity.CENTER_VERTICAL);
        input.setBackground(rounded);
        toolbar.addView(input, new LinearLayout.LayoutParams(0, dp(44), 1));
        field = new EditText(context);
        field.setHint("Search music");
        field.setTextColor(contrast);
        field.setHintTextColor(muted);
        field.setTextSize(16);
        field.setSingleLine(true);
        field.setInputType(android.text.InputType.TYPE_CLASS_TEXT);
        field.setImeOptions(EditorInfo.IME_ACTION_SEARCH | EditorInfo.IME_FLAG_NO_EXTRACT_UI);
        field.setBackground(null);
        field.setPaddingRelative(dp(14), 0, dp(8), 0);
        input.addView(field, new LinearLayout.LayoutParams(0, -1, 1));
        ImageButton clear = icon(R.drawable.search_clear, "Clear search", contrast);
        clear.setVisibility(View.GONE);
        input.addView(clear, new LinearLayout.LayoutParams(dp(44), -1));
        clear.setOnClickListener(view -> field.setText(""));
        ImageButton submit = icon(R.drawable.search_submit, "Search", contrast);
        toolbar.addView(submit, new LinearLayout.LayoutParams(dp(48), dp(48)));
        submit.setOnClickListener(view -> submit(field.getText().toString()));

        recentHeader = new LinearLayout(context);
        recentHeader.setGravity(Gravity.CENTER_VERTICAL);
        recentHeader.setPaddingRelative(dp(20), 0, dp(8), 0);
        TextView heading = new TextView(context);
        heading.setText("Recent searches");
        heading.setTextColor(muted);
        heading.setTextSize(14);
        recentHeader.addView(heading, new LinearLayout.LayoutParams(0, -2, 1));
        Button clearRecent = new Button(context, null, android.R.attr.borderlessButtonStyle);
        clearRecent.setText("Clear");
        clearRecent.setAllCaps(false);
        clearRecent.setContentDescription("Clear recent searches");
        clearRecent.setTextColor(contrast);
        clearRecent.setOnClickListener(view -> {
            recent.clear();
            saveRecent();
            updateRows(new JSONArray());
        });
        recentHeader.addView(clearRecent, new LinearLayout.LayoutParams(-2, dp(48)));
        column.addView(recentHeader);

        ListView suggestions = new ListView(context);
        suggestions.setDivider(null);
        items = new ArrayAdapter<String>(context, android.R.layout.simple_list_item_1) {
            @Override public View getView(int position, View recycled, ViewGroup parent) {
                TextView text = (TextView) super.getView(position, recycled, parent);
                text.setTextColor(contrast);
                text.setTextSize(16);
                text.setSingleLine(true);
                text.setEllipsize(TextUtils.TruncateAt.END);
                text.setPaddingRelative(dp(20), 0, dp(20), 0);
                return text;
            }
        };
        suggestions.setAdapter(items);
        column.addView(suggestions, new LinearLayout.LayoutParams(-1, 0, 1));
        suggestions.setOnItemClickListener((parent, view, position, id) -> submit(items.getItem(position)));
        field.setOnEditorActionListener((view, action, event) -> {
            boolean enter = event != null && event.getKeyCode() == KeyEvent.KEYCODE_ENTER;
            if (action != EditorInfo.IME_ACTION_SEARCH && !enter) return false;
            if (event == null || event.getAction() == KeyEvent.ACTION_UP) submit(field.getText().toString());
            return true;
        });
        field.addTextChangedListener(new TextWatcher() {
            @Override public void beforeTextChanged(CharSequence text, int start, int count, int after) {}
            @Override public void afterTextChanged(Editable editable) {}
            @Override public void onTextChanged(CharSequence text, int start, int before, int count) {
                clear.setVisibility(text.length() == 0 ? View.GONE : View.VISIBLE);
                updateQuery(text.toString().trim());
            }
        });
        setContentView(column);
        Window window = getWindow();
        window.setBackgroundDrawable(new ColorDrawable(background));
        window.setStatusBarColor(background);
        window.setNavigationBarColor(background);
        window.getDecorView().setSystemUiVisibility(light
            ? View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR | View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR : 0);
        window.setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_STATE_ALWAYS_VISIBLE
            | WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE);
        setOnDismissListener(dialog -> {
            if (debounce != null) main.removeCallbacks(debounce);
            finish.accept(selected);
        });
        updateRows(new JSONArray());
    }

    @Override public void show() {
        field.requestFocus();
        super.show();
        getWindow().setLayout(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT);
    }

    void showSuggestions(long revision, String results) {
        if (!isShowing() || this.revision != revision) return;
        try { updateRows(new JSONArray(results)); } catch (JSONException ignored) {}
    }

    private ImageButton icon(int drawable, String label, int tint) {
        Context context = getContext();
        ImageButton button = new ImageButton(context);
        TypedValue background = new TypedValue();
        context.getTheme().resolveAttribute(android.R.attr.selectableItemBackgroundBorderless, background, true);
        button.setBackgroundResource(background.resourceId);
        button.setImageResource(drawable);
        button.setColorFilter(tint);
        button.setContentDescription(label);
        return button;
    }

    private int dp(int points) {
        return Math.round(points * getContext().getResources().getDisplayMetrics().density);
    }

    private void submit(String text) {
        if (text == null || text.trim().isEmpty()) return;
        selected = text.trim();
        recent.removeIf(selected::equalsIgnoreCase);
        recent.add(0, selected);
        if (recent.size() > 15) recent.remove(15);
        saveRecent();
        dismiss();
    }

    private void saveRecent() {
        preferences.edit().putString("recentSearches", new JSONArray(recent).toString()).apply();
    }

    private void updateQuery(String query) {
        if (debounce != null) main.removeCallbacks(debounce);
        this.query = query;
        long current = revision = REVISIONS.incrementAndGet();
        updateRows(new JSONArray());
        if (!query.isEmpty()) {
            debounce = () -> request.send(current, query);
            main.postDelayed(debounce, 240);
        }
    }

    private void updateRows(JSONArray remote) {
        List<String> visible = new ArrayList<>();
        String prefix = query.toLowerCase(Locale.ROOT);
        for (String term : recent) {
            if (term.toLowerCase(Locale.ROOT).startsWith(prefix)) visible.add(term);
        }
        for (int i = 0; i < remote.length() && visible.size() < 10; i++) {
            String term = remote.optString(i);
            if (!term.isEmpty() && visible.stream().noneMatch(term::equalsIgnoreCase)) visible.add(term);
        }
        items.setNotifyOnChange(false);
        items.clear();
        items.addAll(visible);
        items.notifyDataSetChanged();
        recentHeader.setVisibility(prefix.isEmpty() && !visible.isEmpty() ? View.VISIBLE : View.GONE);
    }
}
