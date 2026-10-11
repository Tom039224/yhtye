package io.github.tom039224.yhtye

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.tooling.preview.Preview
import io.github.tom039224.yhtye.shared.APP_NAME

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        setContent {
            YhtyeTheme {
                HomeScreen()
            }
        }
    }
}

@Composable
fun HomeScreen() {
    Scaffold(modifier = Modifier.fillMaxSize()) { innerPadding ->
        Box(
            modifier = Modifier
                .fillMaxSize()
                .padding(innerPadding),
            contentAlignment = Alignment.Center,
        ) {
            Text(text = APP_NAME, style = MaterialTheme.typography.displayMedium)
        }
    }
}

@Preview(showBackground = true)
@Composable
private fun HomeScreenPreview() {
    YhtyeTheme {
        HomeScreen()
    }
}
